use std::collections::HashMap;

use futures::{future::BoxFuture, FutureExt};
use regex::Regex;
use serde::Serialize;
use serde_json::{from_value, to_value};
use std::io::Write;
use tempfile::NamedTempFile;
use tokio::process::Command;
use unidiff::{Hunk, PatchSet, PatchedFile};

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
pub struct GitDiff {
    files: HashMap<String, PatchedFile>,
    hunks: HashMap<Item, Hunk>,
}

impl GitDiff {
    pub fn new() -> Self {
        GitDiff {
            files: HashMap::new(),
            hunks: HashMap::new(),
        }
    }

    pub fn clear(&mut self) {
        self.files.clear();
        self.hunks.clear();
    }

    fn hunk_of_item(&self, item: &Item) -> Result<Hunk, String> {
        let hunk = self.hunks.get(item).ok_or("wow")?.clone();
        Ok(hunk)
    }

    fn patched_file_of_item(&self, item: &Item) -> Result<PatchedFile, String> {
        let patch = self.files.get(item.file()).ok_or("wow")?.clone();
        Ok(patch)
    }

    fn patch_of_item(&self, item: &Item) -> Result<PatchedFile, String> {
        let file = self.patched_file_of_item(item)?;
        let hunk = self.hunk_of_item(item)?;
        let patch_to_stage =
            PatchedFile::with_hunks(file.source_file, file.target_file, vec![hunk]);
        Ok(patch_to_stage)
    }

    fn save_patch_to_temp(&self, item: &Item) -> Result<(NamedTempFile, String), String> {
        let patch_to_stage = self.patch_of_item(item)?;
        let mut temp = NamedTempFile::new().map_err(|e| e.to_string())?;
        writeln!(temp, "{}", patch_to_stage).map_err(|e| e.to_string())?;
        let path = temp.path().to_str().unwrap().to_string();
        Ok((temp, path))
    }

    fn parse_diff(&mut self, kind: HunkKind, diff: String) -> Result<Vec<Item>, String> {
        let mut items = vec![];
        let mut patch = PatchSet::new();
        patch.parse(&diff).map_err(|e| e.to_string())?;
        for patched_file in patch {
            let file = patched_file.target_file.clone();
            if file == "/dev/null" {
                continue;
            }
            let file = file.strip_prefix("b/").unwrap().to_string();
            self.files.insert(file.clone(), patched_file.clone());
            for hunk in patched_file {
                let target_start = hunk.target_start;
                let item = match kind {
                    HunkKind::Staged => Item::Staged {
                        file: file.clone(),
                        target_start,
                    },
                    HunkKind::Unstaged => Item::Unstaged {
                        file: file.clone(),
                        target_start,
                    },
                };
                self.hunks.insert(item.clone(), hunk);
                items.push(item);
            }
        }
        Ok(items)
    }
}

impl ModeDef for GitDiff {
    fn name(&self) -> &'static str {
        "git-diff"
    }
    fn load<'a>(
        &'a mut self,
        _config: &Config,
        _state: &mut State,
        _query: String,
        _item: String,
    ) -> BoxFuture<'a, Result<LoadResp, String>> {
        async move {
            let mut items = vec![];
            self.clear();
            self.parse_diff(HunkKind::Unstaged, git::diff().await?)?
                .iter()
                .for_each(|item| {
                    items.push(item.render());
                });
            self.parse_diff(HunkKind::Staged, git::diff_cached().await?)?
                .iter()
                .for_each(|item| {
                    items.push(item.render());
                });
            git::untracked_files()?
                .into_iter()
                .map(|s| Item::Untracked { file: s })
                .for_each(|item| {
                    items.push(item.render());
                });
            Ok(LoadResp::new_with_default_header(items))
        }
        .boxed()
    }
    fn preview<'a>(
        &'a self,
        _config: &Config,
        _state: &mut State,
        item: String,
    ) -> BoxFuture<'a, Result<PreviewResp, String>> {
        async move {
            let item = Item::parse(&item)?;
            let hunk = self.hunk_of_item(&item)?;
            let message = hunk.colorize();
            Ok(PreviewResp { message })
        }
        .boxed()
    }
    fn execute<'a>(
        &'a mut self,
        _config: &Config,
        state: &'a mut State,
        item: String,
        args: serde_json::Value,
    ) -> BoxFuture<'a, Result<(), String>> {
        async move {
            match from_value(args).map_err(|e| e.to_string())? {
                ExecOpts::Open { tabedit } => {
                    let root = git::get_repo()?
                        .workdir()
                        .ok_or("wow")?
                        .to_owned()
                        .into_os_string()
                        .into_string()
                        .map_err(|_| "wow")?;
                    let item = Item::parse(&item)?;
                    match item {
                        Item::Staged { file, target_start } => {
                            let file = format!("{root}/{file}");
                            let nvim_opts = nvim::OpenOpts {
                                line: Some(target_start),
                                tabedit,
                            };
                            nvim::open(&state.nvim, file.into(), nvim_opts)
                                .await
                                .map_err(|e| e.to_string())?;
                        }
                        Item::Unstaged { file, target_start } => {
                            let file = format!("{root}/{file}");
                            let nvim_opts = nvim::OpenOpts {
                                line: Some(target_start),
                                tabedit,
                            };
                            nvim::open(&state.nvim, file.into(), nvim_opts)
                                .await
                                .map_err(|e| e.to_string())?;
                        }
                        Item::Untracked { file } => {
                            let file = format!("{root}/{file}");
                            let nvim_opts = nvim::OpenOpts {
                                line: None,
                                tabedit,
                            };
                            nvim::open(&state.nvim, file.into(), nvim_opts)
                                .await
                                .map_err(|e| e.to_string())?;
                        }
                    }
                }
                ExecOpts::Stage => {
                    let item = Item::parse(&item)?;
                    match item {
                        Item::Staged { .. } => {
                            // already staged
                        }
                        Item::Unstaged { .. } => {
                            let (__, patch) = self.save_patch_to_temp(&item)?;
                            git_apply(&state.nvim, patch, vec!["--cached"]).await?;
                        }
                        Item::Untracked { file } => {
                            git_add(&state.nvim, file).await?;
                        }
                    }
                }
                ExecOpts::Unstage => {
                    let item = Item::parse(&item)?;
                    if let Item::Staged { .. } = item {
                        let (__, patch) = self.save_patch_to_temp(&item)?;
                        git_apply(&state.nvim, patch, vec!["--reverse", "--cached"]).await?;
                    }
                }
                ExecOpts::Discard => {
                    let item = Item::parse(&item)?;
                    match item {
                        Item::Staged { .. } => {
                            let (__, patch) = self.save_patch_to_temp(&item)?;
                            git_apply(&state.nvim, patch, vec!["--reverse", "--index"]).await?;
                        }
                        Item::Unstaged { .. } => {
                            let (__, patch) = self.save_patch_to_temp(&item)?;
                            git_apply(&state.nvim, patch, vec!["--reverse"]).await?;
                        }
                        Item::Untracked { .. } => {
                            // untracked file cannot be discarded
                        }
                    }
                }
                ExecOpts::Commit => {
                    let _ = nvim::hide_floaterm(&state.nvim).await;
                    let nvim = state.nvim.clone();
                    tokio::spawn(async move {
                        let commit = Command::new("git")
                            .arg("commit")
                            .arg("--verbose")
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .output()
                            .await
                            .map_err(|e| e.to_string());
                        match commit {
                            Ok(output) => {
                                let _ = nvim::notify_command_result(&nvim, "git commit", output)
                                    .await
                                    .map_err(|e| e.to_string());
                            }
                            Err(e) => {
                                let _ = nvim::notify_error(&nvim, &e.to_string())
                                    .await
                                    .map_err(|e| e.to_string());
                            }
                        }
                    });
                }
                ExecOpts::CommitFixup => {
                    let commit = select_commit().await?;
                    let output = Command::new("git")
                        .arg("commit")
                        .arg(format!("--fixup={commit}"))
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .output()
                        .await
                        .map_err(|e| e.to_string())?;
                    nvim::notify_command_result(&state.nvim, "git commit", output)
                        .await
                        .map_err(|e| e.to_string())?;
                }
                ExecOpts::CommitInstantFixup => {
                    let commit = select_commit().await?;
                    let output = Command::new("git")
                        .arg("commit")
                        .arg(format!("--fixup={commit}"))
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .output()
                        .await
                        .map_err(|e| e.to_string())?;
                    nvim::notify_command_result(&state.nvim, "git commit", output)
                        .await
                        .map_err(|e| e.to_string())?;
                    let output = Command::new("git")
                        .arg("rebase")
                        .arg("--update-refs")
                        .arg("--autosquash")
                        .arg("--autostash")
                        .arg("-i")
                        .arg(format!("{}^", commit))
                        .env("GIT_SEQUENCE_EDITOR", ":")
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .output()
                        .await
                        .map_err(|e| e.to_string())?;
                    info!(
                        "git rebase --update-refs --autosquash --autostash -i {}^",
                        commit
                    );
                    nvim::notify_command_result(&state.nvim, "git rebase", output)
                        .await
                        .map_err(|e| e.to_string())?;
                }
            }
            Ok(())
        }
        .boxed()
    }
    fn fzf_bindings(&self) -> (fzf::Bindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [
                execute!(b, |mode,config,state,_query,item| {
                    let opts = ExecOpts::Open { tabedit: false }.value();
                    mode.execute(config, state, item, opts).await
                })
            ],
            "ctrl-t" => [
                execute!(b, |mode,config,state,_query,item| {
                    let opts = ExecOpts::Open { tabedit: false }.value();
                    mode.execute(config, state, item, opts).await
                })
            ],
            "ctrl-s" => [
                execute_silent!(b, |mode,config,state,_query,item| {
                    let opts = ExecOpts::Stage.value();
                    mode.execute(config, state, item, opts).await
                }),
                b.reload()
            ],
            "ctrl-u" => [
                execute_silent!(b, |mode,config,state,_query,item| {
                    let opts = ExecOpts::Unstage.value();
                    mode.execute(config, state, item, opts).await
                }),
                b.reload()
            ],
            "ctrl-x" => [
                execute_silent!(b, |mode,config,state,_query,item| {
                    let opts = ExecOpts::Discard.value();
                    mode.execute(config, state, item, opts).await
                }),
                b.reload()
            ],
            "ctrl-a" => [
                execute_silent!(b, |mode,config,state,_query,item| {
                    let opts = ExecOpts::Commit.value();
                    mode.execute(config, state, item, opts).await
                }),
                b.reload()
            ],
            "ctrl-l" => [
                // TODO git_diff_file に飛ぶ。1行単位でstage/unstageできるようにする
            ],
            "ctrl-space" => [
                select_and_execute!{b, |mode,config,state,_query,item|
                    "commit" => {
                        let opts = ExecOpts::Commit.value();
                        mode.execute(config, state, item, opts).await
                    },
                    "commit(fixup)" => {
                        let opts = ExecOpts::CommitFixup.value();
                        mode.execute(config, state, item, opts).await
                    },
                    "commit(instant fixup)" => {
                        let opts = ExecOpts::CommitInstantFixup.value();
                        mode.execute(config, state, item, opts).await
                    },
                }
            ]
        }
    }
    fn fzf_extra_opts(&self) -> Vec<&str> {
        vec!["--multi", "--preview-window", "right:60%:noborder"]
    }
}

#[derive(Serialize, serde::Deserialize)]
enum ExecOpts {
    Stage,
    Unstage,
    Discard,
    Commit,
    CommitFixup,
    CommitInstantFixup,
    Open { tabedit: bool },
}

impl ExecOpts {
    fn value(&self) -> serde_json::Value {
        to_value(self).unwrap()
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
enum Item {
    Staged { file: String, target_start: usize },
    Unstaged { file: String, target_start: usize },
    Untracked { file: String },
}

impl Item {
    fn file(&self) -> &str {
        match self {
            Item::Staged { file, .. } => file,
            Item::Unstaged { file, .. } => file,
            Item::Untracked { file } => file,
        }
    }

    fn render(&self) -> String {
        match self {
            Item::Staged { file, target_start } => format!(
                "{} {}:{}",
                ansi_term::Colour::Green.bold().paint("S"),
                file,
                target_start
            ),
            Item::Unstaged { file, target_start } => format!(
                "{} {}:{}",
                ansi_term::Colour::Blue.bold().paint("U"),
                file,
                target_start
            ),
            Item::Untracked { file } => {
                format!("{} {}:0", ansi_term::Colour::Red.bold().paint("A"), file)
            }
        }
    }

    fn parse(item: &str) -> Result<Self, String> {
        let (file, target) = item
            .split_once(" ")
            .ok_or("")?
            .1
            .split_once(":")
            .ok_or("")?;
        match item.chars().next().ok_or("")? {
            'S' => Ok(Item::Staged {
                file: file.to_string(),
                target_start: target.parse::<usize>().map_err(|e| e.to_string())?,
            }),
            'U' => Ok(Item::Unstaged {
                file: file.to_string(),
                target_start: target.parse::<usize>().map_err(|e| e.to_string())?,
            }),
            'A' => Ok(Item::Untracked {
                file: file.to_string(),
            }),
            _ => return Err("".to_string()),
        }
    }
}

enum HunkKind {
    Staged,
    Unstaged,
}

trait HunkExt {
    fn colorize(&self) -> String;
}
impl HunkExt for Hunk {
    fn colorize(&self) -> String {
        format!("{}", self)
            .lines()
            .map(|line| {
                if line.starts_with("+") {
                    format!("{}", ansi_term::Colour::Green.paint(line))
                } else if line.starts_with("-") {
                    format!("{}", ansi_term::Colour::Red.paint(line))
                } else {
                    format!("{}", line)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

async fn select_commit() -> Result<String, String> {
    let commits = git::log_graph("--all").await?;
    let commits = commits.iter().map(|s| s.as_str()).collect();
    let commit_line = fzf::select(commits).await?;
    let commit = Regex::new(r"[0-9a-f]{7}")
        .unwrap()
        .find(&commit_line)
        .ok_or("No commit selected")?
        .as_str()
        .to_string();
    Ok(commit)
}

async fn git_add(nvim: &Neovim, file: impl AsRef<str>) -> Result<(), String> {
    let output = Command::new("git")
        .arg("add")
        .arg(file.as_ref())
        .output()
        .await
        .map_err(|e| e.to_string())?;
    nvim::notify_command_result_if_error(nvim, "git add", output)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn git_apply(nvim: &Neovim, patch: String, args: Vec<&str>) -> Result<(), String> {
    let output = Command::new("git")
        .arg("apply")
        .args(args)
        .arg(format!("{}", patch))
        .output()
        .await
        .map_err(|e| e.to_string())?;
    nvim::notify_command_result_if_error(nvim, "git apply", output)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
