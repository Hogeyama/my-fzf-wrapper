use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use encoding_rs::EUC_JP;
use encoding_rs::SHIFT_JIS;
use futures::future::BoxFuture;
use futures::FutureExt;
use git2::Diff;
use git2::Patch;
use serde::Serialize;
use serde_json::from_value;
use serde_json::to_value;
use std::io::Write;
use tempfile::NamedTempFile;
use tokio::process::Command;
use tokio::sync::RwLock;

use crate::config::Config;
use crate::method::LoadResp;
use crate::method::PreviewResp;
use crate::mode::config_builder;
use crate::mode::CallbackMap;
use crate::mode::ModeDef;
use crate::nvim;
use crate::nvim::Neovim;
use crate::nvim::NeovimExt;
use crate::state::State;
use crate::utils::bat;
use crate::utils::fzf;
use crate::utils::fzf::PreviewWindow;
use crate::utils::git;
use crate::utils::vscode;
use crate::utils::xsel;

#[derive(Clone)]
pub struct GitDiff {
    files: Arc<RwLock<HashSet<String>>>,
    hunks: Arc<RwLock<HashMap<Item, Hunk>>>,
}

#[derive(Clone)]
struct Hunk {
    new_file: String,
    target_start: usize,
    patch: Vec<u8>,
}

impl GitDiff {
    pub fn new() -> Self {
        GitDiff {
            files: Arc::new(RwLock::new(HashSet::new())),
            hunks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn clear(&self) {
        self.hunks.write().await.clear();
    }

    async fn hunk_of_item(&self, item: &Item) -> Result<Hunk> {
        let hunk = self
            .hunks
            .read()
            .await
            .get(item)
            .ok_or(anyhow!("wow"))?
            .clone();
        Ok(hunk)
    }

    async fn save_patch_to_temp(&self, item: &Item) -> Result<(NamedTempFile, String)> {
        let hunk = self.hunk_of_item(item).await?;
        let mut temp = NamedTempFile::new()?;
        temp.write_all(&hunk.patch)?;
        let path = temp.path().to_str().unwrap().to_string();
        Ok((temp, path))
    }
}

impl ModeDef for GitDiff {
    fn name(&self) -> &'static str {
        "git-diff"
    }
    fn load<'a>(
        &'a self,
        _config: &Config,
        _state: &mut State,
        _query: String,
        _item: String,
    ) -> super::LoadStream<'a> {
        Box::pin(async_stream::stream! {
            self.clear().await;

            let mut items = vec![];
            let mut files = self.files.write().await;
            let mut hunks = self.hunks.write().await;

            for hunk in git_diff()? {
                let target_start = hunk.target_start;
                let item = Item::UnstagedHunk {
                    file: hunk.new_file.clone(),
                    target_start,
                };
                files.insert(hunk.new_file.clone());
                hunks.insert(item.clone(), hunk);
                items.push(item.render());
            }

            for hunk in git_diff_cached()? {
                let target_start = hunk.target_start;
                let item = Item::StagedHunk {
                    file: hunk.new_file.clone(),
                    target_start,
                };
                files.insert(hunk.new_file.clone());
                hunks.insert(item.clone(), hunk);
                items.push(item.render());
            }

            git::workingtree_modified_files()?
                .into_iter()
                .filter(|s| !files.contains(s))
                .map(|s| Item::UnstagedBinayChange { file: s })
                .for_each(|item| items.push(item.render()));
            git::index_modified_files()?
                .into_iter()
                .filter(|s| !files.contains(s))
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
                .filter(|s| !files.contains(s))
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

            yield Ok(LoadResp::new_with_default_header(items))
        })
    }
    fn preview<'a>(
        &'a self,
        _config: &Config,
        _win: &PreviewWindow,
        item: String,
    ) -> BoxFuture<'a, Result<PreviewResp>> {
        async move {
            let item = Item::parse(&item)?;
            match item {
                Item::StagedHunk { .. } => {
                    let hunk = self.hunk_of_item(&item).await?;
                    let message = hunk.colorize();
                    Ok(PreviewResp { message })
                }
                Item::UnstagedHunk { .. } => {
                    let hunk = self.hunk_of_item(&item).await?;
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
        &'a self,
        config: &'a Config,
        _state: &'a mut State,
        item: String,
        args: serde_json::Value,
    ) -> BoxFuture<'a, Result<()>> {
        async move {
            match from_value(args)? {
                ExecOpts::Open { tabedit } => {
                    let root = git::get_repo()?
                        .workdir()
                        .ok_or(anyhow!("wow"))?
                        .to_owned()
                        .into_os_string()
                        .into_string()
                        .map_err(|_| anyhow!("wow"))?;
                    let item = Item::parse(&item)?;
                    match item {
                        Item::StagedHunk { file, target_start } => {
                            let file = format!("{root}/{file}");
                            let nvim_opts = nvim::OpenOpts {
                                line: Some(target_start),
                                tabedit,
                            };
                            if vscode::in_vscode() {
                                vscode::open(file, None).await?;
                            } else {
                                config.nvim.open(file.into(), nvim_opts).await?;
                            }
                        }
                        Item::UnstagedHunk { file, target_start } => {
                            let file = format!("{root}/{file}");
                            let nvim_opts = nvim::OpenOpts {
                                line: Some(target_start),
                                tabedit,
                            };
                            if vscode::in_vscode() {
                                vscode::open(file, None).await?;
                            } else {
                                config.nvim.open(file.into(), nvim_opts).await?;
                            }
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
                            if vscode::in_vscode() {
                                vscode::open(file, None).await?;
                            } else {
                                config.nvim.open(file.into(), nvim_opts).await?;
                            }
                        }
                        Item::ConflictedFile { file } => {
                            let file = format!("{root}/{file}");
                            let nvim_opts = nvim::OpenOpts {
                                line: None,
                                tabedit,
                            };
                            if vscode::in_vscode() {
                                vscode::open(file, None).await?;
                            } else {
                                config.nvim.open(file.into(), nvim_opts).await?;
                            }
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
                            let (_temp, patch) = self.save_patch_to_temp(&item).await?;
                            git_apply(&config.nvim, patch, vec!["--cached"]).await?;
                        }
                        Item::UnstagedBinayChange { file } => {
                            git_stage_file(&config.nvim, file).await?;
                        }
                        Item::UnstagedFileDeletion { file } => {
                            git_stage_file(&config.nvim, file).await?;
                        }
                        Item::UntrackedFile { file } => {
                            git_stage_file(&config.nvim, file).await?;
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
                            let (_temp, patch) = self.save_patch_to_temp(&item).await?;
                            git_apply(&config.nvim, patch, vec!["--reverse", "--cached"]).await?;
                        }
                        Item::StagedBinayChange { file } => {
                            git_unstage_file(&config.nvim, file).await?;
                        }
                        Item::StagedFileDeletion { file } => {
                            git_unstage_file(&config.nvim, file).await?;
                        }
                        Item::AddedBinaryFile { file } => {
                            git_unstage_file(&config.nvim, file).await?;
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
                    git_stage_file(&config.nvim, file).await?;
                }
                ExecOpts::UnstageFile => {
                    let item = Item::parse(&item)?;
                    let file = item.file();
                    git_unstage_file(&config.nvim, file).await?;
                }
                ExecOpts::Discard => {
                    let item = Item::parse(&item)?;
                    match item {
                        Item::StagedHunk { .. } => {
                            let (_temp, patch) = self.save_patch_to_temp(&item).await?;
                            git_apply(&config.nvim, patch, vec!["--reverse", "--index"]).await?;
                        }
                        Item::UnstagedHunk { .. } => {
                            let (_temp, patch) = self.save_patch_to_temp(&item).await?;
                            git_apply(&config.nvim, patch, vec!["--reverse"]).await?;
                        }
                        Item::StagedBinayChange { .. } => {
                            git_restore_file(&config.nvim, item.file(), Some("HEAD")).await?;
                        }
                        Item::UnstagedBinayChange { .. } => {
                            git_restore_file(&config.nvim, item.file(), None::<&str>).await?;
                        }
                        Item::StagedFileDeletion { .. } => {
                            git_restore_file(&config.nvim, item.file(), Some("HEAD")).await?;
                        }
                        Item::UnstagedFileDeletion { .. } => {
                            git_restore_file(&config.nvim, item.file(), None::<&str>).await?;
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
                    Command::new("git")
                        .arg("commit")
                        .arg("--verbose")
                        .spawn()?
                        .wait()
                        .await?;
                }
                ExecOpts::CommitFixup => {
                    let commit = git::select_commit("target of fixup").await?;
                    let output = Command::new("git")
                        .arg("commit")
                        .arg(format!("--fixup={commit}"))
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .output()
                        .await?;
                    config
                        .nvim
                        .notify_command_result("git commit", output)
                        .await?;
                }
                ExecOpts::CommitInstantFixup => {
                    let commit = git::select_commit("target of instant fixup").await?;
                    let output = Command::new("git")
                        .arg("commit")
                        .arg(format!("--fixup={commit}"))
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .output()
                        .await?;
                    config
                        .nvim
                        .notify_command_result("git commit", output)
                        .await?;
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
                        .await?;
                    info!(
                        "git rebase --update-refs --autosquash --autostash -i {}^",
                        commit
                    );
                    config
                        .nvim
                        .notify_command_result("git rebase", output)
                        .await?;
                }
                ExecOpts::LazyGit => {
                    let pwd = std::env::current_dir().unwrap().into_os_string();
                    Command::new("lazygit")
                        .current_dir(pwd)
                        .spawn()?
                        .wait()
                        .await?;
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
            "ctrl-y" => [
                execute_silent!(b, |_mode,_config,_state,_query,item| {
                    let item = Item::parse(&item)?;
                    xsel::yank(item.file()).await?;
                    Ok(())
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
            "pgup" => [
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

    fn parse(item: &str) -> Result<Self> {
        let (file, target) = item
            .split_once(' ')
            .ok_or(anyhow!(""))?
            .1
            .split_once(':')
            .ok_or(anyhow!(""))?;
        match item.chars().next().ok_or(anyhow!(""))? {
            'S' => Ok(match target.parse::<usize>()? {
                0 => Item::StagedBinayChange {
                    file: file.to_string(),
                },
                n => Item::StagedHunk {
                    file: file.to_string(),
                    target_start: n,
                },
            }),
            'U' => Ok(match target.parse::<usize>()? {
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
            _ => Err(anyhow!("parse error")),
        }
    }
}

trait HunkExt {
    fn colorize(&self) -> String;
}
impl HunkExt for Hunk {
    fn colorize(&self) -> String {
        display_bytes(&self.patch)
            .unwrap_or("Binary File".to_string())
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

// UTF-8, Shift_JIS, EUC-JPで解釈を試みる
fn display_bytes(bytes: &[u8]) -> Option<String> {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return Some(s.to_string());
    }
    let (cow, _, had_errors) = EUC_JP.decode(bytes);
    if !had_errors {
        return Some(cow.into_owned());
    }
    let (cow, _, had_errors) = SHIFT_JIS.decode(bytes);
    if !had_errors {
        return Some(cow.into_owned());
    }
    None
}

fn git_diff() -> Result<Vec<Hunk>> {
    let repo = git::get_repo()?;
    let index = repo.index()?;
    let diff = repo.diff_index_to_workdir(Some(&index), None)?;
    parse_diff(diff)
}

fn git_diff_cached() -> Result<Vec<Hunk>> {
    let repo = git::get_repo()?;
    let index = repo.index()?;
    let head = repo.head()?.peel_to_tree()?;
    let diff = repo.diff_tree_to_index(Some(&head), Some(&index), None)?;
    parse_diff(diff)
}

fn parse_diff(diff: Diff) -> Result<Vec<Hunk>> {
    let mut hunks = vec![];

    for i in 0..diff.deltas().len() {
        let patch = Patch::from_diff(&diff, i).unwrap().unwrap();
        if patch.num_hunks() > 0 {
            for h in 0..patch.num_hunks() {
                if patch.num_lines_in_hunk(h).unwrap() == 0 {
                    info!("empty hunk";
                        "old_file" => patch.delta().old_file().path().unwrap().display(),
                        "new_file" => patch.delta().new_file().path().unwrap().display(),
                    );
                    continue;
                }
                let (hunk, _) = patch.hunk(h).unwrap();
                let mut patch_bytes = vec![];
                patch_bytes.extend_from_slice(
                    format!(
                        "--- a/{}\n",
                        patch.delta().old_file().path().unwrap().display()
                    )
                    .as_bytes(),
                );
                patch_bytes.extend_from_slice(
                    format!(
                        "+++ b/{}\n",
                        patch.delta().new_file().path().unwrap().display()
                    )
                    .as_bytes(),
                );
                patch_bytes.extend_from_slice(hunk.header());
                for l in 0..patch.num_lines_in_hunk(h).unwrap() {
                    if let Ok(line) = patch.line_in_hunk(h, l) {
                        if let c @ ('+' | '-' | ' ') = line.origin() {
                            patch_bytes.push(c as u8);
                        }
                        patch_bytes.extend_from_slice(line.content());
                    }
                }
                hunks.push(Hunk {
                    target_start: hunk.new_start() as usize,
                    new_file: patch
                        .delta()
                        .new_file()
                        .path()
                        .unwrap()
                        .display()
                        .to_string(),
                    patch: patch_bytes,
                });
            }
        }
    }
    Ok(hunks)
}

async fn git_stage_file(nvim: &Neovim, file: impl AsRef<str>) -> Result<()> {
    let output = git::stage_file(file).await?;
    nvim.notify_command_result_if_error("git_stage_file", output)
        .await
}

async fn git_unstage_file(nvim: &Neovim, file: impl AsRef<str>) -> Result<()> {
    let output = git::unstage_file(file).await?;
    nvim.notify_command_result_if_error("git_unstage_file", output)
        .await
}

async fn git_restore_file(
    nvim: &Neovim,
    file: impl AsRef<str>,
    source: Option<impl AsRef<str>>,
) -> Result<()> {
    let output = git::restore_file(file, source).await?;
    nvim.notify_command_result_if_error("git_restore_file", output)
        .await
}

async fn git_apply(nvim: &Neovim, patch: String, args: Vec<&str>) -> Result<()> {
    let output = git::apply(patch, args).await?;
    nvim.notify_command_result_if_error("git apply", output)
        .await
}
