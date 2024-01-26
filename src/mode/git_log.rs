use futures::{future::BoxFuture, FutureExt};
use tokio::process::Command;

use crate::{
    config::Config,
    external_command::{fzf, git, xsel},
    method::{LoadResp, PreviewResp},
    mode::{config_builder, ModeDef},
    nvim::NeovimExt,
    state::State,
};

use super::CallbackMap;

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
        &'a mut self,
        _config: &'a Config,
        _state: &'a mut State,
        _query: String,
        _item: String,
    ) -> BoxFuture<'a, Result<LoadResp, String>> {
        async move {
            let mut commits = match self {
                GitLog::Head => git::log_graph("HEAD").await?,
                GitLog::All => git::log_graph("--all").await?,
            };
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
            "ctrl-l" => [
                execute_silent!{b, |_mode,config,_state,_query,item| {
                    let query = match branches_of(&item) {
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

                    // TODO ad-hoc なので何か考えたい
                    let myself = config.myself.clone();
                    let socket = config.socket.clone();
                    tokio::spawn(async move {
                        let _ = Command::new(myself)
                            .arg("change-mode")
                            .arg("git-branch")
                            .arg(query)
                            .env("FZFW_SOCKET", socket)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .output()
                            .await
                            .map_err(|e| e.to_string());
                    });
                    Ok(())
                }}
            ],
            "ctrl-y" => [
                execute_silent!{b, |_mode,_config,_state,_query,item| {
                    let commit = git::parse_short_commit(&item)?;
                    xsel::yank(commit).await?;
                    Ok(())
                }}
            ],
            "enter" => [
                select_and_execute!{b, |_mode,_config,state,_query,item|
                    "diffview" => {
                        let _ = state.nvim.hide_floaterm().await;
                        state.nvim.command(&format!("DiffviewOpen {}^!", git::parse_short_commit(&item)?))
                            .await
                            .map_err(|e| e.to_string())
                    },
                    "interactive rebase" => {
                        let _ = state.nvim.hide_floaterm().await;
                        let commit = git::parse_short_commit(&item)?;
                        let output = Command::new("git")
                            .arg("rebase")
                            .arg("-i")
                            .arg("--update-refs")
                            .arg("--rebase-merges=rebase-cousins")
                            .arg(commit)
                            .output()
                            .await
                            .map_err(|e| e.to_string())?;
                        state.nvim.notify_command_result("git rebase", output)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    "reset" => {
                        let output = Command::new("git")
                            .arg("reset")
                            .arg(git::parse_short_commit(&item)?)
                            .output()
                            .await
                            .map_err(|e| e.to_string())?;
                        state.nvim.notify_command_result("git reset", output)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    "reset --hard" => {
                        let output = Command::new("git")
                            .arg("reset")
                            .arg("--hard")
                            .arg(git::parse_short_commit(&item)?)
                            .output()
                            .await
                            .map_err(|e| e.to_string())?;
                        state.nvim.notify_command_result("git reset", output)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    "new branch" => {
                        let branch = fzf::input("Enter branch name").await?;
                        let output = Command::new("git")
                            .arg("branch")
                            .arg(branch)
                            .arg(git::parse_short_commit(&item)?)
                            .output()
                            .await
                            .map_err(|e| e.to_string())?;
                        state.nvim.notify_command_result("git branch", output)
                            .await
                            .map_err(|e| e.to_string())
                    }
                },
                b.reload(),
            ],
        }
    }
}

fn branches_of(item: &str) -> Vec<String> {
    let branches = git::parse_branches_of_log(item);
    branches
        .into_iter()
        .filter(|s| !s.starts_with("origin/")) // FIXME ad-hoc
        .collect::<Vec<_>>()
}
