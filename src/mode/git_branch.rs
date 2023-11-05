use futures::{future::BoxFuture, FutureExt};
use regex::Regex;
use tokio::process::Command;

use crate::{
    config::Config,
    external_command::{fzf, git},
    method::{LoadResp, PreviewResp},
    mode::{config_builder, ModeDef},
    nvim::{self, Neovim},
    state::State,
};

use super::CallbackMap;

#[derive(Clone)]
pub struct GitBranch;

impl ModeDef for GitBranch {
    fn name(&self) -> &'static str {
        "git-branch"
    }
    fn load(
        &mut self,
        _config: &Config,
        _state: &mut State,
        _query: String,
        _item: String,
    ) -> BoxFuture<'static, Result<LoadResp, String>> {
        async move {
            let head = git::head()?;
            let mut branches = git::local_branches()?;
            branches.sort_by(|a, b| {
                if a == &head {
                    return std::cmp::Ordering::Less;
                } else if b == &head {
                    return std::cmp::Ordering::Greater;
                } else {
                    return a.cmp(b);
                }
            });
            Ok(LoadResp::new_with_default_header(branches))
        }
        .boxed()
    }
    fn preview(
        &self,
        _config: &Config,
        _state: &mut State,
        branch: String,
    ) -> BoxFuture<'static, Result<PreviewResp, String>> {
        async move {
            let log = git::log_graph(branch).await?;
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
                select_and_execute!{b, |_mode,_config,state,_query,branch|
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
                        push_branch_to_remote(&state.nvim, branch, false).await
                    },
                    "push -f" => {
                        push_branch_to_remote(&state.nvim, branch, true).await
                    }
                }
            ],
        }
    }
}

async fn select_commit(context: impl AsRef<str>) -> Result<String, String> {
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

async fn select_remote(local_branch: impl AsRef<str>) -> Result<String, String> {
    let upstream = git::upstream_of(&local_branch)?;
    let mut branches = git::remote_branches()?
        .into_iter()
        .filter(|b| !b.ends_with("/HEAD"))
        .collect::<Vec<_>>();
    branches.sort_by(|a, b| {
        if a == &upstream {
            return std::cmp::Ordering::Less;
        } else if b == &upstream {
            return std::cmp::Ordering::Greater;
        } else {
            return a.cmp(b);
        }
    });
    let context = format!("pushing {} => ?", local_branch.as_ref());
    fzf::select_with_header(context, branches.iter().map(|s| s.as_str()).collect()).await
}
async fn push_branch_to_remote(nvim: &Neovim, branch: String, force: bool) -> Result<(), String> {
    let remote_ref = select_remote(&branch).await?;
    let (remote, remote_branch) = remote_ref.split_once('/').ok_or("No remote found")?;
    info!("git push -f";
        "temote_branch" => &branch,
        "remote" => &remote,
        "remote_branch" => &remote_branch
    );
    let output = Command::new("git")
        .arg("push")
        .args(if force { vec!["-f"] } else { vec![] })
        .arg(remote)
        .arg(format!("{}:{}", branch, remote_branch))
        .output()
        .await
        .map_err(|e| e.to_string())?;
    nvim::notify_command_result(nvim, "git push", output)
        .await
        .map_err(|e| e.to_string())
}
