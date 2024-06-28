use anyhow::Result;
use futures::future::BoxFuture;
use futures::FutureExt;
use tokio::process::Command;

use crate::config::Config;
use crate::method::LoadResp;
use crate::method::PreviewResp;
use crate::mode::config_builder;
use crate::mode::CallbackMap;
use crate::mode::ModeDef;
use crate::nvim::NeovimExt;
use crate::state::State;
use crate::utils::fzf;
use crate::utils::fzf::PreviewWindow;
use crate::utils::git;

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
    ) -> super::LoadStream {
        Box::pin(async_stream::stream! {
            let mut commits = git::reflog_graph("HEAD").await?;
            // reset color to white
            commits.push(ansi_term::Colour::White.normal().paint("").to_string());
            yield Ok(LoadResp::new_with_default_header(commits))
        })
    }
    fn preview(
        &self,
        _config: &Config,
        _state: &mut State,
        _win: &PreviewWindow,
        item: String,
    ) -> BoxFuture<'static, Result<PreviewResp>> {
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
                            .await?;
                        Ok(())
                    },
                    "cherry-pick" => {
                        let output = Command::new("git")
                            .arg("cherry-pick")
                            .arg(git::parse_short_commit(&item)?)
                            .output()
                            .await?;
                        state.nvim.notify_command_result("git cherry-pick", output)
                            .await?;
                        Ok(())
                    },
                    "switch-detached" => {
                        let output = Command::new("git")
                            .arg("switch")
                            .arg("--detach")
                            .arg(git::parse_short_commit(&item)?)
                            .output()
                            .await?;
                        state.nvim.notify_command_result("git switch --detach", output)
                            .await?;
                        Ok(())
                    },
                }
            ]
        }
    }
}
