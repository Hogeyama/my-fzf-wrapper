use futures::{future::BoxFuture, FutureExt};
use regex::Regex;
use tokio::process::Command;

use crate::{
    external_command::{fzf, git},
    method::{LoadResp, PreviewResp, RunOpts, RunResp},
    nvim,
    types::{Mode, State},
};

#[derive(Clone)]
pub struct GitLog;

impl Mode for GitLog {
    fn new() -> Self {
        GitLog
    }
    fn name(&self) -> &'static str {
        "git-log"
    }
    fn load(
        &self,
        _state: &mut State,
        _opts: Vec<String>,
    ) -> BoxFuture<'static, Result<LoadResp, String>> {
        async move {
            let mut commits = git::log_graph("HEAD").await?;
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
    fn run(
        &self,
        state: &mut State,
        item: String,
        _opts: RunOpts,
    ) -> BoxFuture<'static, Result<RunResp, String>> {
        let nvim = state.nvim.clone();
        async move {
            let commit = Regex::new(r"[0-9a-f]{7}")
                .unwrap()
                .find(&item)
                .ok_or("no commit found")?
                .as_str()
                .to_string();
            let items = vec!["diffview", "interactive-rebase", "reset"];
            match &*fzf::select(items).await? {
                "diffview" => {
                    let _ = nvim::hide_floaterm(&nvim).await;
                    nvim.command(&format!("DiffviewOpen {}^!", commit))
                        .await
                        .map_err(|e| e.to_string())?;
                }
                "interactive-rebase" => {
                    let _ = nvim::hide_floaterm(&nvim).await;
                    let output = Command::new("git")
                        .arg("rebase")
                        .arg("-i")
                        .arg("--update-refs")
                        .arg("--rebase-merges=rebase-cousins")
                        .arg(commit)
                        .output()
                        .await
                        .map_err(|e| e.to_string())?;
                    nvim::notify_command_result(&nvim, "git rebase", output)
                        .await
                        .map_err(|e| e.to_string())?;
                }
                "reset" => {
                    let output = Command::new("git")
                        .arg("reset")
                        .arg(commit)
                        .output()
                        .await
                        .map_err(|e| e.to_string())?;
                    nvim::notify_command_result(&nvim, "git reset", output)
                        .await
                        .map_err(|e| e.to_string())?;
                }
                "reset --hard" => {
                    let output = Command::new("git")
                        .arg("reset")
                        .arg("--hard")
                        .arg(commit)
                        .output()
                        .await
                        .map_err(|e| e.to_string())?;
                    nvim::notify_command_result(&nvim, "git reset --hard", output)
                        .await
                        .map_err(|e| e.to_string())?;
                }
                _ => {}
            }
            Ok(RunResp)
        }
        .boxed()
    }
}
