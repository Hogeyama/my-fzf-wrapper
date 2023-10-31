use futures::{future::BoxFuture, FutureExt};
use regex::Regex;
use tokio::process::Command;

use crate::{
    external_command::{fzf, git},
    method::{LoadResp, PreviewResp},
    mode::{config_builder, ModeDef},
    nvim,
    state::State,
};

use super::CallbackMap;

#[derive(Clone)]
pub struct GitReflog;

impl ModeDef for GitReflog {
    fn new() -> Self {
        GitReflog
    }
    fn name(&self) -> &'static str {
        "git-reflog"
    }
    fn load(
        &mut self,
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
        _state: &mut State,
        item: String,
    ) -> BoxFuture<'static, Result<PreviewResp, String>> {
        async move {
            let commit = Regex::new(r"[0-9a-f]{7}")
                .unwrap()
                .find(&item)
                .ok_or("no commit found")?
                .as_str()
                .to_string();
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
                select_and_execute!{b, |_mode,state,_query,item|
                    "diffview" => {
                        let _ = nvim::hide_floaterm(&state.nvim).await;
                        state.nvim.command(&format!("DiffviewOpen {}^!", commit_of(&item)?))
                            .await
                            .map_err(|e| e.to_string())
                    },
                    "cherry-pick" => {
                        let output = Command::new("git")
                            .arg("cherry-pick")
                            .arg(commit_of(&item)?)
                            .output()
                            .await
                            .map_err(|e| e.to_string())?;
                        nvim::notify_command_result(&state.nvim, "git cherry-pick", output)
                            .await
                            .map_err(|e| e.to_string())
                    },
                }
            ]
        }
    }
}

fn commit_of(item: &str) -> Result<String, String> {
    Regex::new(r"[0-9a-f]{7}")
        .unwrap()
        .find(item)
        .map(|m| m.as_str().to_string())
        .ok_or("no commit found".to_string())
}
