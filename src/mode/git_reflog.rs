use futures::{future::BoxFuture, FutureExt};
use tokio::process::Command;

use crate::{
    config::Config,
    method::{LoadResp, PreviewResp},
    mode::{config_builder, ModeDef},
    nvim::NeovimExt,
    state::State,
    utils::{
        fzf::{self, PreviewWindow},
        git,
    },
};

use super::CallbackMap;

#[derive(Clone)]
pub struct GitReflog;

impl ModeDef for GitReflog {
    fn name(&self) -> &'static str {
        "git-reflog"
    }
    fn load(
        &mut self,
        _config: &Config,
        _state: &mut State,
        _query: String,
        _item: String,
    ) -> BoxFuture<'static, Result<LoadResp, String>> {
        async move {
            let mut commits = git::reflog_graph("HEAD").await?;
            // reset color to white
            commits.push(ansi_term::Colour::White.normal().paint("").to_string());
            Ok(LoadResp::new_with_default_header(commits))
        }
        .boxed()
    }
    fn preview(
        &self,
        _config: &Config,
        _state: &mut State,
        _win: &PreviewWindow,
        item: String,
    ) -> BoxFuture<'static, Result<PreviewResp, String>> {
        async move {
            let commit = git::parse_short_commit(&item)?;
            let message = git::show_commit(commit).await?;
            Ok(PreviewResp { message })
        }
        .boxed()
    }
    fn fzf_bindings(&self) -> (fzf::Bindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [
                select_and_execute!{b, |_mode,_config,state,_query,item|
                    "diffview" => {
                        let _ = state.nvim.hide_floaterm().await;
                        state.nvim.command(&format!("DiffviewOpen {}^!", git::parse_short_commit(&item)?))
                            .await
                            .map_err(|e| e.to_string())
                    },
                    "cherry-pick" => {
                        let output = Command::new("git")
                            .arg("cherry-pick")
                            .arg(git::parse_short_commit(&item)?)
                            .output()
                            .await
                            .map_err(|e| e.to_string())?;
                        state.nvim.notify_command_result("git cherry-pick", output)
                            .await
                            .map_err(|e| e.to_string())
                    },
                }
            ]
        }
    }
}
