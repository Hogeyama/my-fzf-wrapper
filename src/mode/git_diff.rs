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

use super::lib::actions;
use crate::env::Env;
use crate::method::LoadResp;
use crate::method::PreviewResp;
use crate::mode::config_builder;
use crate::mode::CallbackMap;
use crate::mode::ModeDef;
use crate::nvim::Neovim;
use crate::nvim::NeovimExt;
use crate::state::State;
use crate::utils::bat;
use crate::utils::fzf;
use crate::utils::fzf::PreviewWindow;
use crate::utils::git;
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
        _env: &Env,
        _state: &State,
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
                .map(|s| Item::UnstagedBinaryChange { file: s })
                .for_each(|item| items.push(item.render()));
            git::index_modified_files()?
                .into_iter()
                .filter(|s| !files.contains(s))
                .map(|s| Item::StagedBinaryChange { file: s })
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
        _env: &Env,
        _win: &PreviewWindow,
        item: String,
    ) -> BoxFuture<'a, Result<PreviewResp>> {
        async move {
            let item = Item::parse(&item)?;
            let message = if item.is_hunk() {
                self.hunk_of_item(&item).await?.colorize()
            } else if item.is_binary() {
                "binary file".to_string()
            } else if item.is_deletion() {
                if item.is_staged() {
                    "deleted (staged)".to_string()
                } else {
                    "deleted (unstaged)".to_string()
                }
            } else {
                // UntrackedFile, ConflictedFile
                let file = format!("{}{}", git::workdir()?, item.file());
                bat::render_file(&file).await?
            };
            Ok(PreviewResp { message })
        }
        .boxed()
    }
    fn execute<'a>(
        &'a self,
        env: &'a Env,
        _state: &'a State,
        item: String,
        args: serde_json::Value,
    ) -> BoxFuture<'a, Result<()>> {
        async move {
            match from_value(args)? {
                ExecOpts::Open { tabedit, vscode } => {
                    let item = Item::parse(&item)?;
                    if item.can_open() {
                        let root = git::get_repo()?
                            .workdir()
                            .ok_or(anyhow!("wow"))?
                            .to_owned()
                            .into_os_string()
                            .into_string()
                            .map_err(|_| anyhow!("wow"))?;
                        let file = format!("{root}/{}", item.file());
                        if vscode {
                            actions::open_in_vscode(env, file, item.target_start()).await?;
                        } else {
                            actions::open_in_nvim(env, file, item.target_start(), tabedit).await?;
                        }
                    }
                }
                ExecOpts::Stage => {
                    let item = Item::parse(&item)?;
                    if item.can_stage() {
                        if item.is_hunk() {
                            let (_temp, patch) = self.save_patch_to_temp(&item).await?;
                            git_apply(&env.nvim, patch, vec!["--cached"]).await?;
                        } else {
                            git_stage_file(&env.nvim, item.file()).await?;
                        }
                    }
                }
                ExecOpts::Unstage => {
                    let item = Item::parse(&item)?;
                    if item.can_unstage() {
                        if item.is_hunk() {
                            let (_temp, patch) = self.save_patch_to_temp(&item).await?;
                            git_apply(&env.nvim, patch, vec!["--reverse", "--cached"]).await?;
                        } else {
                            git_unstage_file(&env.nvim, item.file()).await?;
                        }
                    }
                }
                ExecOpts::StageFile => {
                    let item = Item::parse(&item)?;
                    let file = item.file();
                    git_stage_file(&env.nvim, file).await?;
                }
                ExecOpts::UnstageFile => {
                    let item = Item::parse(&item)?;
                    let file = item.file();
                    git_unstage_file(&env.nvim, file).await?;
                }
                ExecOpts::Discard => {
                    let item = Item::parse(&item)?;
                    if item.can_discard() {
                        if item.is_hunk() {
                            let (_temp, patch) = self.save_patch_to_temp(&item).await?;
                            let flags = if item.is_staged() {
                                vec!["--reverse", "--index"]
                            } else {
                                vec!["--reverse"]
                            };
                            git_apply(&env.nvim, patch, flags).await?;
                        } else {
                            let source = if item.is_staged() { Some("HEAD") } else { None };
                            git_restore_file(&env.nvim, item.file(), source).await?;
                        }
                    }
                }
                ExecOpts::RestoreOurs => {
                    let item = Item::parse(&item)?;
                    if item.is_conflicted() {
                        let output = git::restore_ours(item.file()).await?;
                        env.nvim
                            .notify_command_result_if_error("git restore --ours", output)
                            .await?;
                    }
                }
                ExecOpts::RestoreTheirs => {
                    let item = Item::parse(&item)?;
                    if item.is_conflicted() {
                        let output = git::restore_theirs(item.file()).await?;
                        env.nvim
                            .notify_command_result_if_error("git restore --theirs", output)
                            .await?;
                    }
                }
                ExecOpts::RestoreMerge => {
                    let item = Item::parse(&item)?;
                    if item.is_conflicted() {
                        let output = git::restore_merge(item.file()).await?;
                        env.nvim
                            .notify_command_result_if_error("git restore --merge", output)
                            .await?;
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
                    env.nvim.notify_command_result("git commit", output).await?;
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
                    env.nvim.notify_command_result("git commit", output).await?;
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
                    env.nvim.notify_command_result("git rebase", output).await?;
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
    fn fzf_bindings(&self) -> (super::ModeBindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [
                execute!(b, |mode,env,state,_query,item| {
                    let opts = ExecOpts::Open { tabedit: false, vscode: false }.value();
                    mode.execute(env, state, item, opts).await
                })
            ],
            "ctrl-t" => [
                execute!(b, |mode,env,state,_query,item| {
                    let opts = ExecOpts::Open { tabedit: false, vscode: false }.value();
                    mode.execute(env, state, item, opts).await
                })
            ],
            "ctrl-s" => [
                execute_silent!(b, |mode,env,state,_query,item| {
                    let opts = ExecOpts::Stage.value();
                    mode.execute(env, state, item, opts).await
                }),
                b.reload()
            ],
            "ctrl-u" => [
                execute_silent!(b, |mode,env,state,_query,item| {
                    let opts = ExecOpts::Unstage.value();
                    mode.execute(env, state, item, opts).await
                }),
                b.reload()
            ],
            "alt-s" => [
                execute_silent!(b, |mode,env,state,_query,item| {
                    let opts = ExecOpts::StageFile.value();
                    mode.execute(env, state, item, opts).await
                }),
                b.reload()
            ],
            "alt-u" => [
                execute_silent!(b, |mode,env,state,_query,item| {
                    let opts = ExecOpts::UnstageFile.value();
                    mode.execute(env, state, item, opts).await
                }),
                b.reload()
            ],
            "ctrl-x" => [
                execute_silent!(b, |mode,env,state,_query,item| {
                    let opts = ExecOpts::Discard.value();
                    mode.execute(env, state, item, opts).await
                }),
                b.reload()
            ],
            "ctrl-y" => [
                execute_silent!(b, |_mode,_env,_state,_query,item| {
                    let item = Item::parse(&item)?;
                    xsel::yank(item.file()).await?;
                    Ok(())
                }),
                b.reload()
            ],
            "ctrl-a" => [
                execute_silent!(b, |mode,env,state,_query,item| {
                    let opts = ExecOpts::Commit.value();
                    mode.execute(env, state, item, opts).await
                }),
                b.reload()
            ],
            "ctrl-v" => [
                execute!(b, |mode,env,state,_query,item| {
                    let opts = ExecOpts::LazyGit.value();
                    mode.execute(env, state, item, opts).await
                }),
                b.reload()
            ],
            "pgup" => [
                select_and_execute!{b, |mode,env,state,_query,item|
                    "commit" => {
                        let opts = ExecOpts::Commit.value();
                        mode.execute(env, state, item, opts).await
                    },
                    "commit(fixup)" => {
                        let opts = ExecOpts::CommitFixup.value();
                        mode.execute(env, state, item, opts).await
                    },
                    "commit(instant fixup)" => {
                        let opts = ExecOpts::CommitInstantFixup.value();
                        mode.execute(env, state, item, opts).await
                    },
                    "vscode" => {
                        let opts = ExecOpts::Open { tabedit: false, vscode: true }.value();
                        mode.execute(env, state, item, opts).await
                    },
                    "restore(ours)", when is_conflicted_item(&item) => {
                        let opts = ExecOpts::RestoreOurs.value();
                        mode.execute(env, state, item, opts).await
                    },
                    "restore(theirs)", when is_conflicted_item(&item) => {
                        let opts = ExecOpts::RestoreTheirs.value();
                        mode.execute(env, state, item, opts).await
                    },
                    "restore(merge)", when is_conflicted_item(&item) => {
                        let opts = ExecOpts::RestoreMerge.value();
                        mode.execute(env, state, item, opts).await
                    },
                }
            ]
        }
    }
    fn mode_enter_actions(&self) -> Vec<fzf::Action> {
        vec![fzf::Action::ChangePreviewWindow(
            "right:60%:noborder".to_string(),
        )]
    }
}

fn is_conflicted_item(item: &str) -> bool {
    Item::parse(item).is_ok_and(|i| i.is_conflicted())
}

#[derive(Serialize, serde::Deserialize)]
enum ExecOpts {
    Stage,
    StageFile,
    Unstage,
    UnstageFile,
    Discard,
    RestoreOurs,
    RestoreTheirs,
    RestoreMerge,
    Commit,
    CommitFixup,
    CommitInstantFixup,
    Open { tabedit: bool, vscode: bool },
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
    StagedBinaryChange { file: String },
    UnstagedBinaryChange { file: String },
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
            Item::StagedHunk { file, .. }
            | Item::UnstagedHunk { file, .. }
            | Item::StagedBinaryChange { file }
            | Item::UnstagedBinaryChange { file }
            | Item::StagedFileDeletion { file }
            | Item::UnstagedFileDeletion { file }
            | Item::AddedBinaryFile { file }
            | Item::UntrackedFile { file }
            | Item::ConflictedFile { file } => file,
        }
    }

    fn is_staged(&self) -> bool {
        matches!(
            self,
            Item::StagedHunk { .. }
                | Item::StagedBinaryChange { .. }
                | Item::StagedFileDeletion { .. }
                | Item::AddedBinaryFile { .. }
        )
    }

    fn is_hunk(&self) -> bool {
        matches!(self, Item::StagedHunk { .. } | Item::UnstagedHunk { .. })
    }

    fn is_binary(&self) -> bool {
        matches!(
            self,
            Item::StagedBinaryChange { .. }
                | Item::UnstagedBinaryChange { .. }
                | Item::AddedBinaryFile { .. }
        )
    }

    fn is_conflicted(&self) -> bool {
        matches!(self, Item::ConflictedFile { .. })
    }

    fn is_deletion(&self) -> bool {
        matches!(
            self,
            Item::StagedFileDeletion { .. } | Item::UnstagedFileDeletion { .. }
        )
    }

    fn target_start(&self) -> Option<usize> {
        match self {
            Item::StagedHunk { target_start, .. } | Item::UnstagedHunk { target_start, .. } => {
                Some(*target_start)
            }
            _ => None,
        }
    }

    fn can_open(&self) -> bool {
        !self.is_binary() && !self.is_deletion()
    }

    fn can_stage(&self) -> bool {
        !self.is_staged()
    }

    fn can_unstage(&self) -> bool {
        self.is_staged()
    }

    fn can_discard(&self) -> bool {
        !matches!(
            self,
            Item::AddedBinaryFile { .. } | Item::UntrackedFile { .. } | Item::ConflictedFile { .. }
        )
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
            Item::StagedBinaryChange { file } => {
                format!("{} {}:0", ansi_term::Colour::Green.bold().paint("S"), file)
            }
            Item::UnstagedBinaryChange { file } => {
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
                0 => Item::StagedBinaryChange {
                    file: file.to_string(),
                },
                n => Item::StagedHunk {
                    file: file.to_string(),
                    target_start: n,
                },
            }),
            'U' => Ok(match target.parse::<usize>()? {
                0 => Item::UnstagedBinaryChange {
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
            'C' => Ok(Item::ConflictedFile {
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
