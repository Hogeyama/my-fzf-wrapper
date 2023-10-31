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
pub struct GitBranch;

impl ModeDef for GitBranch {
    fn new() -> Self {
        GitBranch
    }
    fn name(&self) -> &'static str {
        "git-branch"
    }
    fn load(
        &mut self,
        _state: &mut State,
        _query: String,
        _item: String,
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
    fn fzf_bindings(&self) -> (fzf::Bindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [
                select_and_execute!{b, |_mode,state,_query,branch|
                    "switch" => {
                        let _ = Command::new("git")
                            .arg("switch")
                            .arg("-m")
                            .arg(branch)
                            .output()
                            .await
                            .map_err(|e| e.to_string())?;
                        Ok(())
                    },
                    "repoint" => {
                        let commit = select_commit(format!("select commit to repoint {branch} to"))
                            .await?;
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
                            &state.nvim,
                            format!("git branch {branch} {commit}"),
                            output,
                        )
                        .await
                        .map_err(|e| e.to_string())
                    },
                    "push" => {
                        let remote_ref = select_remote_branch(
                            format!("select remote branch to push {branch} to")
                        ).await?;
                        let (remote, remote_branch) = remote_ref.split_once('/').ok_or("No remote found")?;
                        info!("git push";
                            "temote_branch" => &branch,
                            "remote" => &remote,
                            "remote_branch" => &remote_branch
                        );
                        let output = Command::new("git")
                            .arg("push")
                            .arg(remote)
                            .arg(format!("{}:{}", branch, remote_branch))
                            .output()
                            .await
                            .map_err(|e| e.to_string())?;
                        nvim::notify_command_result(&state.nvim, "git push", output)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    "push -f" => {
                        let remote_ref = select_remote_branch(
                            format!("select remote branch to push {branch} to")
                        ).await?;
                        let (remote, remote_branch) = remote_ref.split_once('/').ok_or("No remote found")?;
                        info!("git push -f";
                            "temote_branch" => &branch,
                            "remote" => &remote,
                            "remote_branch" => &remote_branch
                        );
                        let output = Command::new("git")
                            .arg("push")
                            .arg("-f")
                            .arg(remote)
                            .arg(format!("{}:{}", branch, remote_branch))
                            .output()
                            .await
                            .map_err(|e| e.to_string())?;
                        nvim::notify_command_result(&state.nvim, "git push", output)
                            .await
                            .map_err(|e| e.to_string())
                    }
                }
            ]
        }
    }
}

async fn select_commit(context: impl Into<String>) -> Result<String, String> {
    let commits = git::log_graph("--all").await?;
    let commits = commits.iter().map(|s| s.as_str()).collect();
    let commit_line = fzf::select_with_header(context, commits).await?;
    Ok(Regex::new(r"[0-9a-f]{7}")
        .unwrap()
        .find(&commit_line)
        .ok_or("No commit selected")?
        .as_str()
        .to_string())
}

async fn select_remote_branch(context: impl Into<String>) -> Result<String, String> {
    let branches = git::remote_branches()?;
    let branches: Vec<&str> = branches.iter().map(|s| s.as_str()).collect();
    fzf::select_with_header(context, branches).await
    // let mut candidates = vec!["@{upstream}"];
    // candidates.extend(branches);
    // fzf::select(candidates).await
}
