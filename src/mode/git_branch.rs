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
pub struct GitBranch;

impl Mode for GitBranch {
    fn new() -> Self {
        GitBranch
    }
    fn name(&self) -> &'static str {
        "git-branch"
    }
    fn load(
        &self,
        _state: &mut State,
        _opts: Vec<String>,
    ) -> BoxFuture<'static, Result<LoadResp, String>> {
        async move {
            let branches = Command::new("git")
                .arg("branch")
                .arg("--format=%(refname:short)")
                .output()
                .await
                .map_err(|e| e.to_string())?
                .stdout;
            let branches = String::from_utf8_lossy(branches.as_slice())
                .into_owned()
                .split('\n')
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
                .collect();
            Ok(LoadResp::new_with_default_header(branches))
        }
        .boxed()
    }
    fn preview(
        &self,
        _state: &mut State,
        branch: String,
    ) -> BoxFuture<'static, Result<PreviewResp, String>> {
        async move {
            let log = git::log_graph(branch.clone()).await?;
            let message = log.join("\n");
            Ok(PreviewResp { message })
        }
        .boxed()
    }
    fn run(
        &self,
        state: &mut State,
        branch: String,
        _opts: RunOpts,
    ) -> BoxFuture<'static, Result<RunResp, String>> {
        let nvim = state.nvim.clone();
        async move {
            let items = vec!["switch", "repoint", "cherry-pick", "push", "push -f"];
            match &*fzf::select(items).await? {
                "switch" => {
                    let _ = Command::new("git")
                        .arg("switch")
                        .arg("-m")
                        .arg(branch)
                        .output()
                        .await
                        .map_err(|e| e.to_string())?;
                }
                "repoint" => {
                    let commit = select_commit().await?;
                    let _ = Command::new("git")
                        .arg("branch")
                        .arg("-D")
                        .arg(branch.clone())
                        .output()
                        .await
                        .map_err(|e| e.to_string())?;
                    let output = Command::new("git")
                        .arg("branch")
                        .arg(branch.clone())
                        .arg(commit.clone())
                        .output()
                        .await
                        .map_err(|e| e.to_string())?;
                    nvim::notify_command_result(
                        &nvim,
                        format!("git branch {branch} {commit}"),
                        output,
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                }
                "cherry-pick" => {
                    let commit = select_commit().await?;
                    let output = Command::new("git")
                        .arg("cherry-pick")
                        .arg(commit)
                        .output()
                        .await
                        .map_err(|e| e.to_string())?;
                    nvim::notify_command_result(&nvim, "git cherry-pick", output)
                        .await
                        .map_err(|e| e.to_string())?;
                }
                push if push == "push" || push == "push -f" => {
                    let remote_ref = select_remote_branch().await?;
                    let remote = remote_ref.split('/').nth(0).ok_or("No remote found")?;
                    let remote_branch = remote_ref.split('/').nth(1).ok_or("No branch found")?;
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
                        .map_err(|e| e.to_string())?;
                    nvim::notify_command_result(&nvim, "git push", output)
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

async fn select_commit() -> Result<String, String> {
    let commits = git::log_graph("--all").await?;
    let commits = commits.iter().map(|s| s.as_str()).collect();
    let commit_line = fzf::select(commits).await?;
    Ok(Regex::new(r"[0-9a-f]{7}")
        .unwrap()
        .find(&commit_line)
        .ok_or("No commit selected")?
        .as_str()
        .to_string())
}

async fn select_remote_branch() -> Result<String, String> {
    let branches = git::remote_branches().await?;
    let branches: Vec<&str> = branches.iter().map(|s| s.as_str()).collect();
    fzf::select(branches).await
    // let mut candidates = vec!["@{upstream}"];
    // candidates.extend(branches);
    // fzf::select(candidates).await
}
