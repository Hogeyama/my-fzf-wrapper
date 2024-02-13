use std::collections::HashMap;

use futures::{future::BoxFuture, FutureExt};
use serde::Serialize;
use serde_json::{from_value, to_value};
use std::io::Write;
use tempfile::NamedTempFile;
use tokio::process::Command;
use unidiff::{Hunk, PatchSet, PatchedFile};

use crate::{
    config::Config,
    external_command::{bat, fzf, git},
    method::{LoadResp, PreviewResp},
    mode::{config_builder, ModeDef},
    nvim::{self, Neovim, NeovimExt},
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
                    HunkKind::Staged => Item::StagedHunk {
                        file: file.clone(),
                        target_start,
                    },
                    HunkKind::Unstaged => Item::UnstagedHunk {
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
                .for_each(|item| items.push(item.render()));
            self.parse_diff(HunkKind::Staged, git::diff_cached().await?)?
                .iter()
                .for_each(|item| items.push(item.render()));
            git::workingtree_modified_files()?
                .into_iter()
                .filter(|s| !self.files.contains_key(s))
                .map(|s| Item::UnstagedBinayChange { file: s })
                .for_each(|item| items.push(item.render()));
            git::index_modified_files()?
                .into_iter()
                .filter(|s| !self.files.contains_key(s))
                .map(|s| Item::StagedBinayChange { file: s })
                .for_each(|item| items.push(item.render()));
            git::workingtree_deleted_files()?
                .into_iter()
                .map(|s| Item::UnstagedFileDeletion { file: s })
                .for_each(|item| items.push(item.render()));
            git::index_deleted_files()?
                .into_iter()
                .map(|s| Item::StagedFileDeletion { file: s })
                .for_each(|item| items.push(item.render()));
            git::index_new_files()?
                .into_iter()
                .filter(|s| !self.files.contains_key(s))
                .map(|s| Item::AddedBinaryFile { file: s })
                .for_each(|item| items.push(item.render()));
            git::untracked_files()?
                .into_iter()
                .map(|s| Item::UntrackedFile { file: s })
                .for_each(|item| items.push(item.render()));
            git::conflicted_files()?
                .into_iter()
                .map(|s| Item::ConflictedFile { file: s })
                .for_each(|item| items.push(item.render()));
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
            match item {
                Item::StagedHunk { .. } => {
                    let hunk = self.hunk_of_item(&item)?;
                    let message = hunk.colorize();
                    Ok(PreviewResp { message })
                }
                Item::UnstagedHunk { .. } => {
                    let hunk = self.hunk_of_item(&item)?;
                    let message = hunk.colorize();
                    Ok(PreviewResp { message })
                }
                Item::StagedBinayChange { .. } => {
                    let message = "binary file".to_string();
                    Ok(PreviewResp { message })
                }
                Item::UnstagedBinayChange { .. } => {
                    let message = "binary file".to_string();
                    Ok(PreviewResp { message })
                }
                Item::StagedFileDeletion { .. } => {
                    let message = "deleted (staged)".to_string();
                    Ok(PreviewResp { message })
                }
                Item::UnstagedFileDeletion { .. } => {
                    let message = "deleted (unstaged)".to_string();
                    Ok(PreviewResp { message })
                }
                Item::AddedBinaryFile { .. } => {
                    let message = "binary file".to_string();
                    Ok(PreviewResp { message })
                }
                Item::UntrackedFile { file } => {
                    let file = format!("{}{}", git::workdir()?, file);
                    let message = bat::render_file(&file).await?;
                    Ok(PreviewResp { message })
                }
                Item::ConflictedFile { file } => {
                    info!("ConflictedFile file: {}", file);
                    let message = bat::render_file(&file).await?;
                    Ok(PreviewResp { message })
                }
            }
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
                        Item::StagedHunk { file, target_start } => {
                            let file = format!("{root}/{file}");
                            let nvim_opts = nvim::OpenOpts {
                                line: Some(target_start),
                                tabedit,
                            };
                            state
                                .nvim
                                .open(file.into(), nvim_opts)
                                .await
                                .map_err(|e| e.to_string())?;
                        }
                        Item::UnstagedHunk { file, target_start } => {
                            let file = format!("{root}/{file}");
                            let nvim_opts = nvim::OpenOpts {
                                line: Some(target_start),
                                tabedit,
                            };
                            state
                                .nvim
                                .open(file.into(), nvim_opts)
                                .await
                                .map_err(|e| e.to_string())?;
                        }
                        Item::StagedBinayChange { .. } => {
                            // can't open binary file
                        }
                        Item::UnstagedBinayChange { .. } => {
                            // can't open binary file
                        }
                        Item::StagedFileDeletion { .. } => {
                            // can't open deleted file
                        }
                        Item::UnstagedFileDeletion { .. } => {
                            // can't open deleted file
                        }
                        Item::AddedBinaryFile { .. } => {
                            // can't open binary file
                        }
                        Item::UntrackedFile { file } => {
                            let file = format!("{root}/{file}");
                            let nvim_opts = nvim::OpenOpts {
                                line: None,
                                tabedit,
                            };
                            state
                                .nvim
                                .open(file.into(), nvim_opts)
                                .await
                                .map_err(|e| e.to_string())?;
                        }
                        Item::ConflictedFile { file } => {
                            let file = format!("{root}/{file}");
                            let nvim_opts = nvim::OpenOpts {
                                line: None,
                                tabedit,
                            };
                            state
                                .nvim
                                .open(file.into(), nvim_opts)
                                .await
                                .map_err(|e| e.to_string())?;
                        }
                    }
                }
                ExecOpts::Stage => {
                    let item = Item::parse(&item)?;
                    match item {
                        Item::StagedHunk { .. } => {
                            // already staged
                        }
                        Item::StagedBinayChange { .. } => {
                            // already staged
                        }
                        Item::StagedFileDeletion { .. } => {
                            // already staged
                        }
                        Item::AddedBinaryFile { .. } => {
                            // already staged
                        }
                        Item::UnstagedHunk { .. } => {
                            let (_temp, patch) = self.save_patch_to_temp(&item)?;
                            git_apply(&state.nvim, patch, vec!["--cached"]).await?;
                        }
                        Item::UnstagedBinayChange { file } => {
                            git_stage_file(&state.nvim, file).await?;
                        }
                        Item::UnstagedFileDeletion { file } => {
                            git_stage_file(&state.nvim, file).await?;
                        }
                        Item::UntrackedFile { file } => {
                            git_stage_file(&state.nvim, file).await?;
                        }
                        Item::ConflictedFile { .. } => {
                            // cannot be staged
                        }
                    }
                }
                ExecOpts::Unstage => {
                    let item = Item::parse(&item)?;
                    match item {
                        Item::StagedHunk { .. } => {
                            let (_temp, patch) = self.save_patch_to_temp(&item)?;
                            git_apply(&state.nvim, patch, vec!["--reverse", "--cached"]).await?;
                        }
                        Item::StagedBinayChange { file } => {
                            git_unstage_file(&state.nvim, file).await?;
                        }
                        Item::StagedFileDeletion { file } => {
                            git_unstage_file(&state.nvim, file).await?;
                        }
                        Item::AddedBinaryFile { file } => {
                            git_unstage_file(&state.nvim, file).await?;
                        }
                        Item::UnstagedHunk { .. } => {
                            // already unstaged
                        }
                        Item::UnstagedBinayChange { .. } => {
                            // already unstaged
                        }
                        Item::UnstagedFileDeletion { .. } => {
                            // already unstaged
                        }
                        Item::UntrackedFile { .. } => {
                            // already unstaged
                        }
                        Item::ConflictedFile { .. } => {
                            // cannot be unstaged
                        }
                    }
                }
                ExecOpts::StageFile => {
                    let item = Item::parse(&item)?;
                    let file = item.file();
                    git_stage_file(&state.nvim, file).await?;
                }
                ExecOpts::UnstageFile => {
                    let item = Item::parse(&item)?;
                    let file = item.file();
                    git_unstage_file(&state.nvim, file).await?;
                }
                ExecOpts::Discard => {
                    let item = Item::parse(&item)?;
                    match item {
                        Item::StagedHunk { .. } => {
                            let (_temp, patch) = self.save_patch_to_temp(&item)?;
                            git_apply(&state.nvim, patch, vec!["--reverse", "--index"]).await?;
                        }
                        Item::UnstagedHunk { .. } => {
                            let (_temp, patch) = self.save_patch_to_temp(&item)?;
                            git_apply(&state.nvim, patch, vec!["--reverse"]).await?;
                        }
                        Item::StagedBinayChange { .. } => {
                            git_restore_file(&state.nvim, item.file(), Some("HEAD")).await?;
                        }
                        Item::UnstagedBinayChange { .. } => {
                            git_restore_file(&state.nvim, item.file(), None::<&str>).await?;
                        }
                        Item::StagedFileDeletion { .. } => {
                            git_restore_file(&state.nvim, item.file(), Some("HEAD")).await?;
                        }
                        Item::UnstagedFileDeletion { .. } => {
                            git_restore_file(&state.nvim, item.file(), None::<&str>).await?;
                        }
                        Item::AddedBinaryFile { .. } => {
                            // TODO git rm?
                        }
                        Item::UntrackedFile { .. } => {
                            // untracked file cannot be discarded
                        }
                        Item::ConflictedFile { .. } => {
                            // conflicted file cannot be discarded
                        }
                    }
                }
                ExecOpts::Commit => {
                    let _ = state.nvim.hide_floaterm().await;
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
                                let _ = nvim
                                    .notify_command_result("git commit", output)
                                    .await
                                    .map_err(|e| e.to_string());
                            }
                            Err(e) => {
                                let _ = nvim
                                    .notify_error(&e.to_string())
                                    .await
                                    .map_err(|e| e.to_string());
                            }
                        }
                    });
                }
                ExecOpts::CommitFixup => {
                    let commit = git::select_commit("target of fixup").await?;
                    let output = Command::new("git")
                        .arg("commit")
                        .arg(format!("--fixup={commit}"))
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .output()
                        .await
                        .map_err(|e| e.to_string())?;
                    state
                        .nvim
                        .notify_command_result("git commit", output)
                        .await
                        .map_err(|e| e.to_string())?;
                }
                ExecOpts::CommitInstantFixup => {
                    let commit = git::select_commit("target of instant fixup").await?;
                    let output = Command::new("git")
                        .arg("commit")
                        .arg(format!("--fixup={commit}"))
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .output()
                        .await
                        .map_err(|e| e.to_string())?;
                    state
                        .nvim
                        .notify_command_result("git commit", output)
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
                    state
                        .nvim
                        .notify_command_result("git rebase", output)
                        .await
                        .map_err(|e| e.to_string())?;
                }
                ExecOpts::LazyGit => {
                    let pwd = std::env::current_dir().unwrap().into_os_string();
                    Command::new("lazygit")
                        .current_dir(pwd)
                        .spawn()
                        .map_err(|e| e.to_string())?
                        .wait()
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
            "alt-s" => [
                execute_silent!(b, |mode,config,state,_query,item| {
                    let opts = ExecOpts::StageFile.value();
                    mode.execute(config, state, item, opts).await
                }),
                b.reload()
            ],
            "alt-u" => [
                execute_silent!(b, |mode,config,state,_query,item| {
                    let opts = ExecOpts::UnstageFile.value();
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
            "ctrl-v" => [
                execute!(b, |mode,config,state,_query,item| {
                    let opts = ExecOpts::LazyGit.value();
                    mode.execute(config, state, item, opts).await
                }),
                b.reload()
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
    StageFile,
    Unstage,
    UnstageFile,
    Discard,
    Commit,
    CommitFixup,
    CommitInstantFixup,
    Open { tabedit: bool },
    LazyGit,
}

impl ExecOpts {
    fn value(&self) -> serde_json::Value {
        to_value(self).unwrap()
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
enum Item {
    StagedHunk { file: String, target_start: usize },
    UnstagedHunk { file: String, target_start: usize },
    StagedBinayChange { file: String },
    UnstagedBinayChange { file: String },
    StagedFileDeletion { file: String },
    UnstagedFileDeletion { file: String },
    // TODO
    // StagedRename { old: String, new: String },
    // UnstagedRename { old: String, new: String },
    AddedBinaryFile { file: String }, // BinaryじゃないのはStagedHunkに入る
    UntrackedFile { file: String },
    ConflictedFile { file: String },
}

impl Item {
    fn file(&self) -> &str {
        match self {
            Item::StagedHunk { file, .. } => file,
            Item::UnstagedHunk { file, .. } => file,
            Item::StagedBinayChange { file } => file,
            Item::UnstagedBinayChange { file } => file,
            Item::StagedFileDeletion { file } => file,
            Item::UnstagedFileDeletion { file } => file,
            Item::AddedBinaryFile { file } => file,
            Item::UntrackedFile { file } => file,
            Item::ConflictedFile { file } => file,
        }
    }

    fn render(&self) -> String {
        match self {
            Item::StagedHunk { file, target_start } => format!(
                "{} {}:{}",
                ansi_term::Colour::Green.bold().paint("S"),
                file,
                target_start
            ),
            Item::UnstagedHunk { file, target_start } => format!(
                "{} {}:{}",
                ansi_term::Colour::Blue.bold().paint("U"),
                file,
                target_start
            ),
            Item::StagedBinayChange { file } => {
                format!("{} {}:0", ansi_term::Colour::Green.bold().paint("S"), file)
            }
            Item::UnstagedBinayChange { file } => {
                format!("{} {}:0", ansi_term::Colour::Blue.bold().paint("U"), file)
            }
            Item::StagedFileDeletion { file } => {
                format!("{} {}:0", ansi_term::Colour::Green.bold().paint("D"), file)
            }
            Item::UnstagedFileDeletion { file } => {
                format!("{} {}:0", ansi_term::Colour::Red.bold().paint("d"), file)
            }
            Item::AddedBinaryFile { file } => {
                format!("{} {}:0", ansi_term::Colour::Green.bold().paint("A"), file)
            }
            Item::UntrackedFile { file } => {
                format!("{} {}:0", ansi_term::Colour::Red.bold().paint("a"), file)
            }
            Item::ConflictedFile { file } => {
                format!("{} {}:0", ansi_term::Colour::Yellow.bold().paint("C"), file)
            }
        }
    }

    fn parse(item: &str) -> Result<Self, String> {
        let (file, target) = item
            .split_once(' ')
            .ok_or("")?
            .1
            .split_once(':')
            .ok_or("")?;
        match item.chars().next().ok_or("")? {
            'S' => Ok(match target.parse::<usize>().map_err(|e| e.to_string())? {
                0 => Item::StagedBinayChange {
                    file: file.to_string(),
                },
                n => Item::StagedHunk {
                    file: file.to_string(),
                    target_start: n,
                },
            }),
            'U' => Ok(match target.parse::<usize>().map_err(|e| e.to_string())? {
                0 => Item::UnstagedBinayChange {
                    file: file.to_string(),
                },
                n => Item::UnstagedHunk {
                    file: file.to_string(),
                    target_start: n,
                },
            }),
            'A' => Ok(Item::AddedBinaryFile {
                file: file.to_string(),
            }),
            'a' => Ok(Item::UntrackedFile {
                file: file.to_string(),
            }),
            'D' => Ok(Item::StagedFileDeletion {
                file: file.to_string(),
            }),
            'd' => Ok(Item::UnstagedFileDeletion {
                file: file.to_string(),
            }),
            'C' => Ok(Item::UntrackedFile {
                file: file.to_string(),
            }),
            _ => Err("parse error".to_string()),
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
                if line.starts_with('+') {
                    format!("{}", ansi_term::Colour::Green.paint(line))
                } else if line.starts_with('-') {
                    format!("{}", ansi_term::Colour::Red.paint(line))
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

async fn git_stage_file(nvim: &Neovim, file: impl AsRef<str>) -> Result<(), String> {
    let output = git::stage_file(file).await?;
    nvim.notify_command_result_if_error("git_stage_file", output)
        .await
        .map_err(|e| e.to_string())
}

async fn git_unstage_file(nvim: &Neovim, file: impl AsRef<str>) -> Result<(), String> {
    let output = git::unstage_file(file).await?;
    nvim.notify_command_result_if_error("git_unstage_file", output)
        .await
        .map_err(|e| e.to_string())
}

async fn git_restore_file(
    nvim: &Neovim,
    file: impl AsRef<str>,
    source: Option<impl AsRef<str>>,
) -> Result<(), String> {
    let output = git::restore_file(file, source).await?;
    nvim.notify_command_result_if_error("git_restore_file", output)
        .await
        .map_err(|e| e.to_string())
}

async fn git_apply(nvim: &Neovim, patch: String, args: Vec<&str>) -> Result<(), String> {
    let output = git::apply(patch, args).await?;
    nvim.notify_command_result_if_error("git apply", output)
        .await
        .map_err(|e| e.to_string())
}
