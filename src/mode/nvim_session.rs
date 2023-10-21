use std::fs;

use futures::{future::BoxFuture, FutureExt};

use crate::{
    external_command::fzf,
    method::{Load, LoadResp, Method, PreviewResp, RunOpts, RunResp},
    nvim::{self, Neovim},
    types::{Mode, State},
};

#[derive(Clone)]
pub struct NeovimSession;

pub fn new() -> NeovimSession {
    NeovimSession
}
impl Mode for NeovimSession {
    fn name(&self) -> &'static str {
        "neovim_session"
    }
    fn load(
        &mut self,
        _state: &mut State,
        _opts: Vec<String>,
    ) -> BoxFuture<'static, <Load as Method>::Response> {
        async move {
            let home = std::env::var("HOME").unwrap();
            // わざわざ tokio::fs にしなくていいかな
            let sessions = fs::read_dir(format!("{home}/.local/share/nvim/sessions"))
                .unwrap()
                .filter_map(|x| x.ok().and_then(|x| x.file_name().into_string().ok()))
                .collect::<Vec<String>>();

            // .into_iter()
            // .collect();
            LoadResp::new_with_default_header(sessions)
        }
        .boxed()
    }
    fn preview(&mut self, _state: &mut State, _item: String) -> BoxFuture<'static, PreviewResp> {
        async move {
            PreviewResp {
                message: "No description".to_string(),
            }
        }
        .boxed()
    }
    fn run(
        &mut self,
        state: &mut State,
        session: String,
        _opts: RunOpts,
    ) -> BoxFuture<'static, RunResp> {
        let nvim = state.nvim.clone();
        async move {
            let items = vec!["switch", "delete"];
            async fn possesion(nvim: &Neovim, action: &str, session: String) {
                let _ = nvim::hide_floaterm(&nvim).await;
                let r = nvim::eval_lua(
                    &nvim,
                    format!("require(\"nvim-possession\").{action}({{\"{session}\"}})"),
                )
                .await
                .map_err(|e| e.to_string());
                match r {
                    Ok(_) => {
                        let _ = nvim::notify_info(
                            &nvim,
                            format!("possession.{action}(\"{session}\") succeeded"),
                        )
                        .await;
                    }
                    Err(e) => {
                        let _ = nvim::notify_error(
                            &nvim,
                            format!("possession.{action}(\"{session}\") failed: {e}"),
                        )
                        .await;
                    }
                }
            }
            match &*fzf::select(items).await {
                "switch" => {
                    possesion(&nvim, "load", session).await;
                }
                "delete" => {
                    possesion(&nvim, "delete_selected", session).await;
                }
                _ => {}
            }
            RunResp
        }
        .boxed()
    }
}
