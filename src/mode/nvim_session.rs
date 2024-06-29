use std::fs;

use anyhow::Result;
use futures::future::BoxFuture;
use futures::FutureExt;

use crate::bindings;
use crate::config::Config;
use crate::method::LoadResp;
use crate::method::PreviewResp;
use crate::mode::config_builder;
use crate::mode::CallbackMap;
use crate::mode::ModeDef;
use crate::nvim::Neovim;
use crate::nvim::NeovimExt;
use crate::state::State;
use crate::utils::fzf;
use crate::utils::fzf::PreviewWindow;

#[derive(Clone)]
pub struct NeovimSession;

impl ModeDef for NeovimSession {
    fn name(&self) -> &'static str {
        "neovim-session"
    }
    fn fzf_bindings(&self) -> (fzf::Bindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [
                select_and_execute!{b, |_mode,config,_state,_query,session|
                    "switch" => {
                        session_command(&config.nvim, "read", session).await;
                        Ok(())
                    },
                    "delete" => {
                        session_command(&config.nvim, "delete", session).await;
                        Ok(())
                    },
                }
            ]
        }
    }
    fn load(
        &mut self,
        _config: &Config,
        _state: &mut State,
        _query: String,
        _item: String,
    ) -> super::LoadStream {
        Box::pin(async_stream::stream! {
            let home = std::env::var("HOME").unwrap();
            // わざわざ tokio::fs にしなくていいかな
            let sessions = fs::read_dir(format!("{home}/.local/share/nvim/session"))?
                .filter_map(|x| x.ok().and_then(|x| x.file_name().into_string().ok()))
                .collect::<Vec<String>>();
            yield Ok(LoadResp::new_with_default_header(sessions))
        })
    }
    fn preview(
        &self,
        _config: &Config,
        _win: &PreviewWindow,
        _item: String,
    ) -> BoxFuture<'static, Result<PreviewResp>> {
        async move {
            Ok(PreviewResp {
                message: "No description".to_string(),
            })
        }
        .boxed()
    }
}

async fn session_command(nvim: &Neovim, action: &str, session: String) {
    let _ = nvim.hide_floaterm().await;
    let r = nvim
        .eval_lua(format!(
            "require(\"mini.sessions\").{action}(\"{session}\")"
        ))
        .await
        .map_err(|e| e.to_string());
    match r {
        Ok(_) => {
            let _ = nvim
                .notify_info(format!("mini.sessions.{action}(\"{session}\") succeeded"))
                .await;
        }
        Err(e) => {
            let _ = nvim
                .notify_error(format!("mini.sessions.{action}(\"{session}\") failed: {e}"))
                .await;
        }
    }
}
