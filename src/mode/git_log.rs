use anyhow::anyhow;
use anyhow::Result;
use futures::future::BoxFuture;
use futures::FutureExt;
use tokio::process::Command;

use crate::env::Env;
use crate::method::LoadResp;
use crate::method::PreviewResp;
use crate::mode;
use crate::mode::config_builder;
use crate::mode::CallbackMap;
use crate::mode::ModeDef;
use crate::nvim::Neovim;
use crate::nvim::NeovimExt;
use crate::state::State;
use crate::utils::fzf;
use crate::utils::fzf::PreviewWindow;
use crate::utils::git;
use crate::utils::xsel;

#[derive(Clone)]
pub enum GitLog {
    Head,
    All,
}

impl ModeDef for GitLog {
    fn name(&self) -> &'static str {
        match self {
            GitLog::Head => "git-log",
            GitLog::All => "git-log(all)",
        }
    }
    fn load<'a>(
        &'a self,
        _env: &'a Env,
        _state: &'a mut State,
        _query: String,
        _item: String,
    ) -> super::LoadStream<'a> {
        Box::pin(async_stream::stream! {
            let mut commits = match self {
                GitLog::Head => git::log_graph("HEAD").await?,
                GitLog::All => git::log_graph("--all").await?,
            };
            // reset color to white
            commits.push(ansi_term::Colour::White.normal().paint("").to_string());
            yield Ok(LoadResp::new_with_default_header(commits))
        })
    }
    fn preview(
        &self,
        _env: &Env,
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
            "ctrl-l" => [
                execute_silent!{b, |_mode,env,state,_query,item| {
                    let query = match branches_of(&item)? {
                        branches if branches.is_empty() => {
                            "".to_string()
                        }
                        branches if branches.len() == 1 => {
                            branches[0].clone()
                        }
                        branches => {
                            fzf::select_with_header(
                                "select branch",
                                branches.iter().map(|b| b.as_str()).collect(),
                            ).await?
                        }
                    };
                    // git-branch に切り替え、選択したブランチを query にセット
                    mode::do_change_mode(env, state, "git-branch", !query.is_empty()).await?;
                    if !query.is_empty() {
                        env.fzf_client.post_actions(&[
                            fzf::Action::ClearQuery,
                            fzf::Action::Raw(format!("change-query({})", query)),
                        ]).await?;
                    }
                    Ok(())
                }}
            ],
            "ctrl-y" => [
                execute_silent!{b, |_mode,_env,_state,_query,item| {
                    let commit = git::parse_short_commit(&item)?;
                    xsel::yank(commit).await?;
                    Ok(())
                }}
            ],
            "enter" => [
                select_and_execute!{b, |_mode,env,_state,_query,item|
                    "diffview" => {
                        let _ = env.nvim.hide_floaterm().await;
                        env.nvim.command(&format!("DiffviewOpen {}^!", git::parse_short_commit(&item)?))
                            .await?;
                        Ok(())
                    },
                    "interactive rebase" => {
                        let _ = env.nvim.hide_floaterm().await;
                        let commit = git::parse_short_commit(&item)?;
                        let output = Command::new("git")
                            .arg("rebase")
                            .arg("-i")
                            .arg("--update-refs")
                            .arg("--rebase-merges=no-rebase-cousins")
                            .arg(format!("{}^", commit))
                            .output()
                            .await?;
                        env.nvim.notify_command_result("git rebase", output)
                            .await
                    },
                    "reset" => {
                        let output = Command::new("git")
                            .arg("reset")
                            .arg(git::parse_short_commit(&item)?)
                            .output()
                            .await?;
                        env.nvim.notify_command_result("git reset", output)
                            .await
                    },
                    "reset --hard" => {
                        let output = Command::new("git")
                            .arg("reset")
                            .arg("--hard")
                            .arg(git::parse_short_commit(&item)?)
                            .output()
                            .await?;
                        env.nvim.notify_command_result("git reset", output)
                            .await
                    },
                    "reword" => {
                        let _ = env.nvim.hide_floaterm().await;
                        let commit = git::parse_short_commit(&item)?;
                        let output = Command::new("git")
                            .env("GIT_SEQUENCE_EDITOR", r"sed '0,/^\(p\|pick\) /s/^\(p\|pick\) /reword /' -i")
                            .arg("rebase")
                            .arg("-i")
                            .arg("--update-refs")
                            .arg("--rebase-merges=rebase-cousins")
                            .arg(format!("{}^", commit))
                            .output()
                            .await?;
                        env.nvim.notify_command_result("git rebase", output)
                            .await
                    },
                    "push to remote" => {
                        push_to_remote(&env.nvim, &item, false).await
                    },
                    "push to remote (force)" => {
                        push_to_remote(&env.nvim, &item, true).await
                    },
                    "revert" => {
                        let output = Command::new("git")
                            .arg("revert")
                            .arg(git::parse_short_commit(&item)?)
                            .output()
                            .await?;
                        env.nvim.notify_command_result("git revert", output)
                            .await
                    },
                    "new branch" => {
                        let branch = fzf::input("Enter branch name").await?;
                        let output = Command::new("git")
                            .arg("branch")
                            .arg(branch)
                            .arg(git::parse_short_commit(&item)?)
                            .output()
                            .await?;
                        env.nvim.notify_command_result("git branch", output)
                            .await
                    },
                    "switch-detached" => {
                        let output = Command::new("git")
                            .arg("switch")
                            .arg("--detach")
                            .arg(git::parse_short_commit(&item)?)
                            .output()
                            .await?;
                        env.nvim.notify_command_result("git switch --detach", output)
                            .await?;
                        Ok(())
                    },
                },
                b.reload(),
            ],
        }
    }
    fn wants_sort(&self) -> bool {
        false
    }
}

async fn push_to_remote(nvim: &Neovim, item: &String, force: bool) -> Result<()> {
    let commit = git::parse_short_commit(item)?;
    let all_remote_branches = git::remote_branches()?;
    let preferred_branches = branches_of(item)?
        .into_iter()
        .filter(|b| all_remote_branches.contains(b)) // remove local branch
        .collect::<Vec<_>>();
    let branches = preferred_branches
        .iter()
        .chain(
            all_remote_branches
                .iter()
                .filter(|b| !preferred_branches.contains(b)),
        )
        .map(|b| b.as_str())
        .collect::<Vec<_>>();
    let selected_branch = fzf::select_with_header("branch to push to", branches).await?;
    let (remote, selected_branch) = selected_branch
        .split_once('/')
        .ok_or(anyhow!("No remote found"))?;
    let output = git::push(remote, commit, selected_branch, force).await?;
    nvim.notify_command_result("git push", output).await
}

fn branches_of(item: &str) -> Result<Vec<String>> {
    let branches = git::parse_branches_of_log(item);
    let remotes = git::remotes()?;
    Ok(branches
        .into_iter()
        .filter(|s| remotes.iter().all(|r| !s.starts_with(&format!("{}/", r))))
        .collect::<Vec<_>>())
}
