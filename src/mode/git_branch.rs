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
            let branches = Command::new("git")
                .arg("branch")
                .arg("--format=%(refname:short)")
                .output()
                .await
                .map_err(|e| e.to_string())
                .unwrap()
                .stdout;
            let branches = String::from_utf8_lossy(branches.as_slice()).into_owned();
            let branches = branches
                .split('\n')
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
                .collect();
            LoadResp::new_with_default_header(branches)
        }
        .boxed()
    }
    fn preview(&mut self, _state: &mut State, branch: String) -> BoxFuture<'static, PreviewResp> {
        async move {
            let log = git::log_graph(branch.clone()).await;
            let message = log.join("\n");
            PreviewResp { message }
        }
        .boxed()
    }
    fn run(
        &mut self,
        state: &mut State,
        branch: String,
        _opts: RunOpts,
    ) -> BoxFuture<'static, RunResp> {
        let nvim = state.nvim.clone();
        async move {
            let items = vec!["switch", "repoint", "push", "push -f"];
            match &*fzf::select(items).await {
                "switch" => {
                    let _ = Command::new("git")
                        .arg("switch")
                        .arg("-m")
                        .arg(branch)
                        .output()
                        .await
                        .map_err(|e| e.to_string())
                        .unwrap();
                }
                "repoint" => {
                    let commit = select_commit().await;
                    let _ = Command::new("git")
                        .arg("branch")
                        .arg("-D")
                        .arg(branch.clone())
                        .output()
                        .await
                        .map_err(|e| e.to_string())
                        .unwrap();
                    let _ = Command::new("git")
                        .arg("branch")
                        .arg(branch)
                        .arg(commit)
                        .output()
                        .await
                        .map_err(|e| e.to_string())
                        .unwrap();
                }
                "cherry-pick" => {
                    let commit = select_commit().await;
                    let output = Command::new("git")
                        .arg("cherry-pick")
                        .arg(commit)
                        .output()
                        .await
                        .map_err(|e| e.to_string())
                        .unwrap();
                    nvim::notify_command_result(&nvim, "git cherry-pick", output).await;
                }
                push if push == "push" || push == "push -f" => {
                    let remote_ref = select_remote_branch().await;
                    let remote = remote_ref.split('/').nth(0).unwrap();
                    let remote_branch = remote_ref.split('/').nth(1).unwrap();
                    let output = Command::new("git")
                        .args(if push == "push" {
                            vec!["push"]
                        } else {
                            vec!["push", "-f"]
                        })
                        .arg(remote)
                        .arg(format!("{}:{}", branch, remote_branch))
                        .output()
                        .await
                        .map_err(|e| e.to_string())
                        .unwrap();
                    nvim::notify_command_result(&nvim, "git push", output).await;
                }
                _ => {}
            }
            RunResp
        }
        .boxed()
    }
}

async fn select_commit() -> String {
    let commits = git::log_graph("--all").await;
    let commits = commits.iter().map(|s| s.as_str()).collect();
    let commit_line = fzf::select(commits).await;
    Regex::new(r"[0-9a-f]{7}")
        .unwrap()
        .find(&commit_line)
        .unwrap()
        .as_str()
        .to_string()
}

async fn select_remote_branch() -> String {
    let branches = git::remote_branches().await;
    let branches: Vec<&str> = branches.iter().map(|s| s.as_str()).collect();
    fzf::select(branches).await
    // let mut candidates = vec!["@{upstream}"];
    // candidates.extend(branches);
    // fzf::select(candidates).await
}
