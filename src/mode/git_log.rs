use futures::{future::BoxFuture, FutureExt};
use regex::Regex;
use tokio::process::Command;

use crate::{
    external_command::{fzf, git},
    method::{Load, LoadResp, Method, PreviewResp, RunOpts, RunResp},
    nvim,
    types::{Mode, State},
};

#[derive(Clone)]
pub struct GitBranch;

pub fn new() -> GitBranch {
    GitBranch
}
impl Mode for GitBranch {
    fn name(&self) -> &'static str {
        "git_branch"
    }
    fn load(
        &mut self,
        _state: &mut State,
        _opts: Vec<String>,
    ) -> BoxFuture<'static, <Load as Method>::Response> {
        async move {
            LoadResp::new_with_default_header(commits)
            let commits = git::log_graph("HEAD").await;
        }
        .boxed()
    }
    fn preview(&mut self, _state: &mut State, item: String) -> BoxFuture<'static, PreviewResp> {
        async move {
            let commit = Regex::new(r"[0-9a-f]{7}")
                .unwrap()
                .find(&item)
                .unwrap()
                .as_str()
                .to_string();
            let message = git::show_commit(commit).await;
            PreviewResp { message }
        }
        .boxed()
    }
    fn run(
        &mut self,
        state: &mut State,
        item: String,
        _opts: RunOpts,
    ) -> BoxFuture<'static, RunResp> {
        let nvim = state.nvim.clone();
        async move {
            let commit = Regex::new(r"[0-9a-f]{7}")
                .unwrap()
                .find(&item)
                .unwrap()
                .as_str()
                .to_string();
            let items = vec!["interactive-rebase"];
            match &*fzf::select(items).await {
                "interactive-rebase" => {
                    let _ = nvim::hide_floaterm(&nvim).await;
                    let _ = Command::new("git")
                        .arg("rebase")
                        .arg("-i")
                        .arg("--update-refs")
                        .arg("--rebase-merges=rebase-cousins")
                        .arg(commit)
                        .output()
                        .await
                        .map_err(|e| e.to_string())
                        .unwrap();
                }
                _ => {}
            }
            RunResp
        }
        .boxed()
    }
}
