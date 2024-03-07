use std::fs;

use futures::{future::BoxFuture, FutureExt};

use crate::{
    bindings,
    config::Config,
    method::{LoadResp, PreviewResp},
    mode::{config_builder, ModeDef},
    nvim::{Neovim, NeovimExt},
    state::State,
    utils::fzf,
};

use super::CallbackMap;

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
                select_and_execute!{b, |_mode,_config,state,_query,session|
                    "switch" => {
                        session_command(&state.nvim, "read", session).await;
                        Ok(())
                    },
                    "delete" => {
                        session_command(&state.nvim, "delete", session).await;
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
    ) -> BoxFuture<'static, Result<LoadResp, String>> {
        async move {
            let home = std::env::var("HOME").unwrap();
            // わざわざ tokio::fs にしなくていいかな
            let sessions = fs::read_dir(format!("{home}/.local/share/nvim/session"))
                .map_err(|e| e.to_string())?
                .filter_map(|x| x.ok().and_then(|x| x.file_name().into_string().ok()))
                .collect::<Vec<String>>();
            Ok(LoadResp::new_with_default_header(sessions))
        }
        .boxed()
    }
    fn preview(
        &self,
        _config: &Config,
        _state: &mut State,
        _item: String,
    ) -> BoxFuture<'static, Result<PreviewResp, String>> {
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
