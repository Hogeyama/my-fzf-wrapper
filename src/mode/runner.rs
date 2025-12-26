use std::sync::Arc;

use anyhow::{anyhow, Result};
use futures::future::BoxFuture;
use futures::FutureExt;
use regex::Regex;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::process::Command;
use tokio::sync::Mutex;

use crate::config::Config;
use crate::method::LoadResp;
use crate::method::PreviewResp;
use crate::mode::config_builder;
use crate::mode::CallbackMap;
use crate::mode::ModeDef;
use crate::nvim::NeovimExt;
// use crate::state::State; // Conflict with local State
use crate::mode::fd as mode_fd;
use crate::utils::command::edit_and_run;
use crate::utils::fd;
use crate::utils::fzf;
use crate::utils::fzf::PreviewWindow;
use std::process::Output;

#[derive(Clone)]
pub struct State {
    pub target_file: Option<String>,
}

impl State {
    pub fn new() -> Self {
        Self { target_file: None }
    }
}

pub fn new_modes() -> (Runner, RunnerCommands) {
    let state = Arc::new(Mutex::new(State::new()));
    (Runner::new(state.clone()), RunnerCommands::new(state))
}

#[derive(Clone)]
pub struct Runner {
    state: Arc<Mutex<State>>,
}

impl Runner {
    pub fn new(state: Arc<Mutex<State>>) -> Self {
        Self { state }
    }
}

impl ModeDef for Runner {
    fn name(&self) -> &'static str {
        "runner"
    }

    fn load(
        &self,
        _config: &Config,
        _state: &mut crate::state::State,
        _query: String,
        _item: String,
    ) -> super::LoadStream {
        let mut cmd = fd::new();
        cmd.arg("-H")
            .arg("-t")
            .arg("f")
            .arg("(Makefile|justfile|build.gradle)");
        mode_fd::load(cmd)
    }

    fn preview(
        &self,
        _config: &Config,
        _win: &PreviewWindow,
        item: String,
    ) -> BoxFuture<'static, Result<PreviewResp>> {
        async move {
            let commands = parse_commands(&item).await?;
            let message = commands.join("\n");
            Ok(PreviewResp { message })
        }
        .boxed()
    }

    fn fzf_bindings(&self) -> (fzf::Bindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [
                {
                    let state = self.state.clone();
                    b.execute_silent(move |_mode, _config, _state, _query, item| {
                        let state = state.clone();
                        async move {
                            state.lock().await.target_file = Some(item);
                            Ok(())
                        }.boxed()
                    })
                },
                b.change_mode("runner_commands", false),
            ],
        }
    }
}

#[derive(Clone)]
pub struct RunnerCommands {
    state: Arc<Mutex<State>>,
}

impl RunnerCommands {
    pub fn new(state: Arc<Mutex<State>>) -> Self {
        Self { state }
    }
}

impl ModeDef for RunnerCommands {
    fn name(&self) -> &'static str {
        "runner_commands"
    }

    fn load(
        &self,
        _config: &Config,
        _state: &mut crate::state::State,
        _query: String,
        _item: String,
    ) -> super::LoadStream {
        let state = self.state.clone();
        Box::pin(async_stream::stream! {
            let items = match state.lock().await.target_file.clone() {
                Some(file) => match parse_commands(&file).await {
                    Ok(commands) => commands,
                    Err(e) => vec![format!("Error: {}", e)],
                },
                None => vec!["Error: No file selected".to_string()],
            };
            yield Ok(LoadResp::new_with_default_header(items));
        })
    }

    fn preview(
        &self,
        _config: &Config,
        _win: &PreviewWindow,
        _item: String,
    ) -> BoxFuture<'static, Result<PreviewResp>> {
        async move {
            Ok(PreviewResp {
                message: "".to_string(),
            })
        }
        .boxed()
    }

    fn fzf_bindings(&self) -> (fzf::Bindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [{
                let state = self.state.clone();
                b.execute(move |_mode, config, _state, _query, item| {
                     let state = state.clone();
                     async move {
                         let file = state.lock().await.target_file.clone().ok_or(anyhow!("no file"))?;
                         let (cmd, output) = run_target(&file, &item).await?;
                         config.nvim.notify_command_result(&cmd, output).await
                     }.boxed()
                })
            }],
            "pgup" => [{
                let state = self.state.clone();
                b.execute(move |_mode, config, _state, _query, item| {
                     let state = state.clone();
                     async move {
                        match &*fzf::select(vec!["execute", "execute with arguments"]).await? {
                            "execute" => {
                                let file = state.lock().await.target_file.clone().ok_or(anyhow!("no file"))?;
                                let (cmd, output) = run_target(&file, &item).await?;
                                config.nvim.notify_command_result(&cmd, output).await?;
                                Ok(())
                            },
                            "execute with arguments" => {
                                let file = state.lock().await.target_file.clone().ok_or(anyhow!("no file"))?;
                                let cmd = build_command(&file, &item);
                                let (cmd, output) = edit_and_run(cmd).await?;
                                config.nvim.notify_command_result(&cmd, output).await?;
                                Ok(())
                            },
                            _ => Ok(()),
                        }
                     }.boxed()
                })
            }],
        }
    }
}

async fn parse_commands(path: &str) -> Result<Vec<String>> {
    let file = tokio::fs::File::open(path).await?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    let mut commands = Vec::new();

    let path_str = path.to_string();
    if path_str.ends_with("Makefile") {
        let re = Regex::new(r"^([a-zA-Z0-9_-]+):")?;
        while let Some(line) = lines.next_line().await? {
            if let Some(caps) = re.captures(&line) {
                if let Some(target) = caps.get(1) {
                    commands.push(target.as_str().to_string());
                }
            }
        }
    } else if path_str.ends_with("justfile") {
        let output = Command::new("just")
            .arg("--list")
            .arg("--justfile")
            .arg(path)
            .output()
            .await?;
        let stdout = String::from_utf8(output.stdout)?;
        let re = Regex::new(r"^\s*([a-zA-Z0-9_-]+)")?;
        for line in stdout.lines().skip(1) {
            // Skip "Available recipes:"
            if let Some(caps) = re.captures(line) {
                if let Some(target) = caps.get(1) {
                    commands.push(target.as_str().to_string());
                }
            }
        }
    } else if path_str.ends_with("build.gradle") {
        let re = Regex::new(r"task\s+([a-zA-Z0-9_-]+)")?;
        while let Some(line) = lines.next_line().await? {
            if let Some(caps) = re.captures(&line) {
                if let Some(target) = caps.get(1) {
                    commands.push(target.as_str().to_string());
                }
            }
        }
    }

    Ok(commands)
}

fn build_command(file: &str, target: &str) -> String {
    if file.ends_with("Makefile") {
        format!("make -f {} {}", file, target)
    } else if file.ends_with("justfile") {
        format!("just --justfile {} {}", file, target)
    } else if file.ends_with("build.gradle") {
        format!("gradle -b {} {}", file, target)
    } else {
        format!("echo 'Unknown build file: {}'", file)
    }
}

async fn run_target(file: &str, target: &str) -> Result<(String, Output)> {
    let cmd_str = build_command(file, target);
    let output = Command::new("sh").arg("-c").arg(&cmd_str).output().await?;
    Ok((cmd_str, output))
}
