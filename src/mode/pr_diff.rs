use anyhow::anyhow;
use anyhow::Result;
use futures::future::BoxFuture;
use futures::FutureExt;
use serde::Deserialize;
use tokio::process::Command;

use super::lib::actions;
use super::lib::cache::ModeCache;
use crate::env::Env;
use crate::method::LoadResp;
use crate::method::PreviewResp;
use crate::mode::config_builder;
use crate::mode::CallbackMap;
use crate::mode::ModeAction;
use crate::mode::ModeDef;
use crate::nvim::NeovimExt;
use crate::utils::fzf;
use crate::utils::fzf::PreviewWindow;
use crate::utils::git;
use crate::utils::xsel;

#[derive(Clone)]
pub struct PrDiff {
    hunks: ModeCache<Vec<DiffHunk>>,
    pr_meta: ModeCache<PrMeta>,
    pending_comments: ModeCache<PersistedPendingComments>,
}

impl PrDiff {
    pub fn new() -> Self {
        Self {
            hunks: ModeCache::new(),
            pr_meta: ModeCache::new(),
            pending_comments: ModeCache::new(),
        }
    }

    async fn pending_count(&self) -> usize {
        self.pending_comments
            .with(|v| v.comments.len())
            .await
            .unwrap_or(0)
    }

    fn prompt_with_pending(count: usize) -> String {
        if count > 0 {
            format!("pr-diff ({count} pending)>")
        } else {
            "pr-diff>".to_string()
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
struct PendingComment {
    path: String,
    body: String,
    line: Option<usize>,
    side: Option<String>,
    start_line: Option<usize>,
    start_side: Option<String>,
    is_file_comment: bool,
}

/// Wrapper for persisting pending comments with stale detection.
/// The `head_sha` is compared on restore to detect if the PR head has changed.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct PersistedPendingComments {
    head_sha: String,
    comments: Vec<PendingComment>,
}

#[derive(Clone, Debug)]
struct PrMeta {
    number: u64,
    owner: String,
    repo: String,
    head_sha: String,
}

#[derive(Clone, Debug)]
struct DiffHunk {
    file_path: String,
    #[allow(dead_code)]
    hunk_header: String,
    new_start: usize,
    lines: Vec<DiffLine>,
    raw_text: String,
    rename_from: Option<String>,
}

#[derive(Clone, Debug)]
struct DiffLine {
    kind: DiffLineKind,
    content: String,
    old_lineno: Option<usize>,
    new_lineno: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum DiffLineKind {
    Added,
    Removed,
    Context,
}

impl ModeDef for PrDiff {
    fn name(&self) -> &'static str {
        "pr-diff"
    }

    fn load<'a>(&'a self, _env: &'a Env, _query: String, _item: String) -> super::LoadStream<'a> {
        Box::pin(async_stream::stream! {
            let meta = fetch_pr_meta().await?;
            let diff_text = fetch_pr_diff().await?;
            let hunks = parse_unified_diff(&diff_text);
            let items: Vec<String> = hunks
                .iter()
                .enumerate()
                .map(|(i, h)| render_hunk_item(i, h))
                .collect();
            self.hunks.set(hunks).await;

            // Enable persistence for pending comments using PR identity
            let ns = format!("pr-diff_{}_{}", meta.owner, meta.repo);
            let key = format!("pending-comments_{}", meta.number);
            if let Err(e) = self.pending_comments.enable_persistence(&ns, &key).await {
                crate::warn!("failed to enable persistence for pending comments: {}", e);
            }

            // Initialize or restore pending_comments
            if let Ok(persisted) = self.pending_comments.get().await {
                // Restored from file -- check staleness
                if persisted.head_sha != meta.head_sha {
                    _env.nvim
                        .notify_warn("Pending comments may be stale: PR head has changed")
                        .await
                        .ok();
                }
            } else {
                self.pending_comments
                    .set(PersistedPendingComments {
                        head_sha: meta.head_sha.clone(),
                        comments: vec![],
                    })
                    .await;
            }

            self.pr_meta.set(meta).await;
            yield Ok(LoadResp::new_with_default_header(items));
        })
    }

    fn preview<'a>(
        &'a self,
        _env: &Env,
        _win: &PreviewWindow,
        item: String,
    ) -> BoxFuture<'a, Result<PreviewResp>> {
        async move {
            let idx = parse_hunk_index(&item)?;
            let message = self
                .hunks
                .with(|hunks| hunks.get(idx).map(colorize_hunk))
                .await
                .ok()
                .flatten()
                .unwrap_or_else(|| "(hunk not found)".to_string());
            Ok(PreviewResp { message })
        }
        .boxed()
    }

    fn fzf_bindings(&self) -> (super::ModeBindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [
                execute!(b, |_mode, env, _query, item| {
                    let idx = parse_hunk_index(&item)?;
                    let (file, line) = _mode
                        .hunks
                        .with(|hunks| {
                            hunks.get(idx).map(|h| (h.file_path.clone(), h.new_start))
                        })
                        .await
                        .ok()
                        .flatten()
                        .ok_or_else(|| anyhow!("hunk not found"))?;
                    let workdir = git::workdir()?;
                    let full_path = format!("{}{}", workdir, file);
                    actions::open_in_nvim(env, full_path, Some(line), false).await
                })
            ],
            "ctrl-y" => [
                execute_silent!(b, |_mode, _env, _query, item| {
                    let idx = parse_hunk_index(&item)?;
                    let file = _mode
                        .hunks
                        .with(|hunks| hunks.get(idx).map(|h| h.file_path.clone()))
                        .await
                        .ok()
                        .flatten()
                        .ok_or_else(|| anyhow!("hunk not found"))?;
                    xsel::yank(&file).await?;
                    Ok(())
                })
            ],
            "pgup" => [
                execute!(b, |mode, env, _query, item| {
                    let action = fzf::select(vec![
                        "comment",
                        "comment-file",
                        "submit",
                        "pending",
                        "discard",
                        "browse",
                    ]).await?;
                    match &*action {
                        "comment" => {
                            let idx = parse_hunk_index(&item)?;
                            let hunk = mode
                                .hunks
                                .with(|hunks| hunks.get(idx).cloned())
                                .await
                                .ok()
                                .flatten()
                                .ok_or_else(|| anyhow!("hunk not found"))?;
                            add_pending_comment_flow(mode, &hunk).await?;
                            let count = mode.pending_count().await;
                            env.nvim.notify_info(
                                format!("Added to pending ({count} comments)")
                            ).await?;
                        }
                        "comment-file" => {
                            let idx = parse_hunk_index(&item)?;
                            let file_path = mode
                                .hunks
                                .with(|hunks| hunks.get(idx).map(|h| h.file_path.clone()))
                                .await
                                .ok()
                                .flatten()
                                .ok_or_else(|| anyhow!("hunk not found"))?;
                            add_pending_file_comment_flow(mode, &file_path).await?;
                            let count = mode.pending_count().await;
                            env.nvim.notify_info(
                                format!("Added to pending ({count} comments)")
                            ).await?;
                        }
                        "submit" => {
                            let meta = mode.pr_meta.get().await?;
                            submit_review_flow(mode, env, &meta).await?;
                        }
                        "pending" => {
                            show_pending_comments(mode).await?;
                        }
                        "discard" => {
                            let head_sha = mode.pr_meta.with(|m| m.head_sha.clone()).await
                                .unwrap_or_default();
                            mode.pending_comments
                                .set(PersistedPendingComments {
                                    head_sha,
                                    comments: vec![],
                                })
                                .await;
                            mode.pending_comments.delete_file().await;
                            env.nvim.notify_info("Pending comments discarded").await?;
                        }
                        "browse" => {
                            let meta = mode.pr_meta.get().await?;
                            let url = format!(
                                "https://github.com/{}/{}/pull/{}/files",
                                meta.owner, meta.repo, meta.number
                            );
                            Command::new("xdg-open")
                                .arg(&url)
                                .stdin(std::process::Stdio::null())
                                .stdout(std::process::Stdio::null())
                                .stderr(std::process::Stdio::null())
                                .spawn()?
                                .wait()
                                .await?;
                        }
                        _ => {}
                    }
                    // Update prompt to reflect pending count
                    let count = mode.pending_count().await;
                    env.post_fzf_actions(&[
                        ModeAction::Fzf(fzf::Action::ChangePrompt(
                            PrDiff::prompt_with_pending(count),
                        )),
                    ]).await?;
                    Ok(())
                })
            ],
        }
    }

    fn wants_sort(&self) -> bool {
        false
    }

    fn mode_enter_actions(&self) -> Vec<fzf::Action> {
        vec![fzf::Action::ChangePreviewWindow(
            "right:60%:noborder".to_string(),
        )]
    }
}

// ---------------------------------------------------------------------------
// PR metadata
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhPrView {
    number: u64,
    head_ref_oid: String,
}

async fn fetch_pr_meta() -> Result<PrMeta> {
    let pr_output = Command::new("gh")
        .args(["pr", "view", "--json", "number,headRefOid"])
        .output()
        .await?;
    if !pr_output.status.success() {
        let stderr = String::from_utf8_lossy(&pr_output.stderr);
        return Err(anyhow!("gh pr view failed: {}", stderr));
    }
    let pr: GhPrView = serde_json::from_slice(&pr_output.stdout)?;

    let (owner, repo) = current_repo_owner_name().await?;

    Ok(PrMeta {
        number: pr.number,
        owner,
        repo,
        head_sha: pr.head_ref_oid,
    })
}

async fn current_repo_owner_name() -> Result<(String, String)> {
    #[derive(Deserialize)]
    struct RepoView {
        owner: RepoOwner,
        name: String,
    }
    #[derive(Deserialize)]
    struct RepoOwner {
        login: String,
    }

    let output = Command::new("gh")
        .args(["repo", "view", "--json", "owner,name"])
        .output()
        .await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("gh repo view failed: {}", stderr));
    }
    let repo: RepoView = serde_json::from_slice(&output.stdout)?;
    Ok((repo.owner.login, repo.name))
}

async fn fetch_pr_diff() -> Result<String> {
    let output = Command::new("gh").args(["pr", "diff"]).output().await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("gh pr diff failed: {}", stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

// ---------------------------------------------------------------------------
// Diff parser
// ---------------------------------------------------------------------------

fn parse_unified_diff(diff: &str) -> Vec<DiffHunk> {
    let mut hunks = Vec::new();
    let mut current_file: Option<String> = None;
    let mut rename_from: Option<String> = None;
    let mut has_hunks = false;

    let lines: Vec<&str> = diff.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        if line.starts_with("diff --git ") {
            // Flush rename-only entry from previous file section
            if let (Some(ref new_path), Some(ref old_path)) = (&current_file, &rename_from) {
                if !has_hunks {
                    hunks.push(DiffHunk {
                        file_path: new_path.clone(),
                        hunk_header: String::new(),
                        new_start: 0,
                        lines: vec![],
                        raw_text: format!("rename from {}\nrename to {}", old_path, new_path),
                        rename_from: Some(old_path.clone()),
                    });
                }
            }
            current_file = None;
            rename_from = None;
            has_hunks = false;
            i += 1;
            continue;
        }

        if let Some(path) = line.strip_prefix("rename from ") {
            rename_from = Some(path.to_string());
            i += 1;
            continue;
        }

        if let Some(path) = line.strip_prefix("rename to ") {
            current_file = Some(path.to_string());
            i += 1;
            continue;
        }

        if let Some(path) = line.strip_prefix("+++ b/") {
            current_file = Some(path.to_string());
            i += 1;
            continue;
        }

        if line.starts_with("+++ /dev/null") {
            current_file = Some("/dev/null".to_string());
            i += 1;
            continue;
        }

        if line.starts_with("--- ") {
            i += 1;
            continue;
        }

        if line.starts_with("@@ ") {
            let file_path = match &current_file {
                Some(f) => f.clone(),
                None => {
                    i += 1;
                    continue;
                }
            };

            if let Some((new_start, _new_lines, old_start)) = parse_hunk_header(line) {
                has_hunks = true;
                let hunk_header = line.to_string();
                let mut hunk_lines = Vec::new();
                let mut raw_lines = vec![line.to_string()];
                let mut old_lineno = old_start;
                let mut new_lineno = new_start;

                i += 1;
                while i < lines.len() {
                    let l = lines[i];
                    if l.starts_with("diff --git ")
                        || l.starts_with("@@ ")
                        || l.starts_with("+++ ")
                        || l.starts_with("--- ")
                    {
                        break;
                    }

                    raw_lines.push(l.to_string());

                    if let Some(rest) = l.strip_prefix('+') {
                        hunk_lines.push(DiffLine {
                            kind: DiffLineKind::Added,
                            content: rest.to_string(),
                            old_lineno: None,
                            new_lineno: Some(new_lineno),
                        });
                        new_lineno += 1;
                    } else if let Some(rest) = l.strip_prefix('-') {
                        hunk_lines.push(DiffLine {
                            kind: DiffLineKind::Removed,
                            content: rest.to_string(),
                            old_lineno: Some(old_lineno),
                            new_lineno: None,
                        });
                        old_lineno += 1;
                    } else if let Some(rest) = l.strip_prefix(' ') {
                        hunk_lines.push(DiffLine {
                            kind: DiffLineKind::Context,
                            content: rest.to_string(),
                            old_lineno: Some(old_lineno),
                            new_lineno: Some(new_lineno),
                        });
                        old_lineno += 1;
                        new_lineno += 1;
                    } else {
                        // No newline at end of file marker etc.
                        raw_lines.pop();
                        i += 1;
                        continue;
                    }

                    i += 1;
                }

                hunks.push(DiffHunk {
                    file_path,
                    hunk_header,
                    new_start,
                    lines: hunk_lines,
                    raw_text: raw_lines.join("\n"),
                    rename_from: rename_from.clone(),
                });
                continue;
            }
        }

        // Binary files, index lines, etc.
        i += 1;
    }

    // Flush last file section if rename-only
    if let (Some(ref new_path), Some(ref old_path)) = (&current_file, &rename_from) {
        if !has_hunks {
            hunks.push(DiffHunk {
                file_path: new_path.clone(),
                hunk_header: String::new(),
                new_start: 0,
                lines: vec![],
                raw_text: format!("rename from {}\nrename to {}", old_path, new_path),
                rename_from: Some(old_path.clone()),
            });
        }
    }

    hunks
}

fn parse_hunk_header(header: &str) -> Option<(usize, usize, usize)> {
    // @@ -old_start,old_lines +new_start,new_lines @@
    let header = header.strip_prefix("@@ ")?;
    let at_end = header.find(" @@")?;
    let range_part = &header[..at_end];

    let mut parts = range_part.split(' ');
    let old_part = parts.next()?.strip_prefix('-')?;
    let new_part = parts.next()?.strip_prefix('+')?;

    let old_start = if let Some((start, _)) = old_part.split_once(',') {
        start.parse().ok()?
    } else {
        old_part.parse().ok()?
    };

    let (new_start, new_lines) = if let Some((start, lines)) = new_part.split_once(',') {
        (start.parse().ok()?, lines.parse().ok()?)
    } else {
        (new_part.parse().ok()?, 1)
    };

    Some((new_start, new_lines, old_start))
}

// ---------------------------------------------------------------------------
// Item rendering / parsing
// ---------------------------------------------------------------------------

fn render_hunk_item(index: usize, hunk: &DiffHunk) -> String {
    let rename_prefix = hunk
        .rename_from
        .as_ref()
        .map(|from| format!("{} → ", from))
        .unwrap_or_default();

    if hunk.lines.is_empty() {
        // Rename-only (no content changes)
        return format!("{}{}  (renamed) |{}", rename_prefix, hunk.file_path, index);
    }

    let additions = hunk
        .lines
        .iter()
        .filter(|l| l.kind == DiffLineKind::Added)
        .count();
    let deletions = hunk
        .lines
        .iter()
        .filter(|l| l.kind == DiffLineKind::Removed)
        .count();
    format!(
        "{}{}:{} (+{}/-{}) |{}",
        rename_prefix, hunk.file_path, hunk.new_start, additions, deletions, index
    )
}

fn parse_hunk_index(item: &str) -> Result<usize> {
    item.rsplit('|')
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow!("failed to parse hunk index from: {}", item))
}

fn colorize_hunk(hunk: &DiffHunk) -> String {
    hunk.raw_text
        .lines()
        .map(|line| {
            if line.starts_with('+') && !line.starts_with("+++") {
                format!("{}", ansi_term::Colour::Green.paint(line))
            } else if line.starts_with('-') && !line.starts_with("---") {
                format!("{}", ansi_term::Colour::Red.paint(line))
            } else if line.starts_with("@@") {
                format!("{}", ansi_term::Colour::Cyan.paint(line))
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Comment range calculation
// ---------------------------------------------------------------------------

/// Determine the side (LEFT/RIGHT) and line range for a PR review comment
/// based on which diff lines were selected.
///
/// - All removed lines → LEFT side, using old_lineno.
/// - Mixed or all added/context → RIGHT side, using new_lineno.
///   Removed lines lack new_lineno, so the range is expanded by searching
///   outward from the selected indices to find the nearest new_lineno.
fn compute_comment_range(
    lines: &[DiffLine],
    selected_indices: &[usize],
) -> Result<(&'static str, usize, usize)> {
    let selected_lines: Vec<&DiffLine> = selected_indices.iter().map(|&i| &lines[i]).collect();

    let all_removed = selected_lines
        .iter()
        .all(|l| l.kind == DiffLineKind::Removed);

    if all_removed {
        let line_numbers: Vec<usize> = selected_lines.iter().filter_map(|l| l.old_lineno).collect();
        let start = *line_numbers.iter().min().unwrap();
        let end = *line_numbers.iter().max().unwrap();
        Ok(("LEFT", start, end))
    } else {
        let min_idx = *selected_indices.iter().min().unwrap();
        let max_idx = *selected_indices.iter().max().unwrap();

        let start = (0..=min_idx)
            .rev()
            .find_map(|i| lines[i].new_lineno)
            .or_else(|| (min_idx..lines.len()).find_map(|i| lines[i].new_lineno))
            .ok_or_else(|| anyhow!("no new_lineno found for start"))?;

        let end = (max_idx..lines.len())
            .find_map(|i| lines[i].new_lineno)
            .or_else(|| (0..=max_idx).rev().find_map(|i| lines[i].new_lineno))
            .ok_or_else(|| anyhow!("no new_lineno found for end"))?;

        Ok(("RIGHT", start, end))
    }
}

// ---------------------------------------------------------------------------
// Pending comment flows
// ---------------------------------------------------------------------------

async fn add_pending_comment_flow(mode: &PrDiff, hunk: &DiffHunk) -> Result<()> {
    // 1. Format hunk lines for nvim buffer
    let diff_lines: Vec<String> = format_diff_lines_for_buffer(&hunk.lines);
    let marker = "=".repeat(40);
    let buffer_content = format!("\n{marker}\n{}", diff_lines.join("\n"));

    // 2. Write buffer and Lua script to temp files
    let tmp_buffer = tempfile::Builder::new().suffix(".md").tempfile()?;
    std::fs::write(tmp_buffer.path(), &buffer_content)?;

    let lua_script = generate_comment_lua_script();
    let tmp_lua = tempfile::Builder::new().suffix(".lua").tempfile()?;
    std::fs::write(tmp_lua.path(), &lua_script)?;

    // 3. Open nvim with the buffer and Lua script
    Command::new("nvimw")
        .arg("--tmux-popup")
        .arg(tmp_buffer.path())
        .arg("-c")
        .arg(format!("luafile {}", tmp_lua.path().display()))
        .spawn()?
        .wait()
        .await?;

    // 4. Parse result
    let content = std::fs::read_to_string(tmp_buffer.path())?;
    let (body, buf_start, buf_end) = match parse_nvim_comment_buffer(&content) {
        Some(v) => v,
        None => return Ok(()), // cancelled or no range selected
    };

    if body.is_empty() {
        return Ok(());
    }

    // 5. Convert buffer line range (1-based) to hunk indices (0-based)
    let selected_indices: Vec<usize> = (buf_start - 1..buf_end).collect();
    if selected_indices.is_empty()
        || selected_indices.last().copied().unwrap_or(0) >= hunk.lines.len()
    {
        return Ok(());
    }

    let (side, start_line, end_line) = compute_comment_range(&hunk.lines, &selected_indices)?;

    // 6. Add to pending
    let comment = PendingComment {
        path: hunk.file_path.clone(),
        body,
        line: Some(end_line),
        side: Some(side.to_string()),
        start_line: if start_line != end_line {
            Some(start_line)
        } else {
            None
        },
        start_side: if start_line != end_line {
            Some(side.to_string())
        } else {
            None
        },
        is_file_comment: false,
    };
    mode.pending_comments
        .with_mut(|v| v.comments.push(comment))
        .await?;

    Ok(())
}

/// Format diff lines for display in the nvim comment buffer.
/// Each line: `{old:>4} {new:>4} {prefix}{content}`
fn format_diff_lines_for_buffer(lines: &[DiffLine]) -> Vec<String> {
    lines
        .iter()
        .map(|l| {
            let old = l
                .old_lineno
                .map(|n| format!("{:>4}", n))
                .unwrap_or_else(|| "    ".to_string());
            let new = l
                .new_lineno
                .map(|n| format!("{:>4}", n))
                .unwrap_or_else(|| "    ".to_string());
            let prefix = match l.kind {
                DiffLineKind::Added => "+",
                DiffLineKind::Removed => "-",
                DiffLineKind::Context => " ",
            };
            format!("{} {} {}{}", old, new, prefix, l.content)
        })
        .collect()
}

/// Generate the Lua script for the nvim comment buffer.
///
/// Buffer layout:
///   Line 1:  (empty, comment area - user writes here)
///   Line 2:  ========================================
///   Line 3+: diff lines
///
/// Workflow:
///   1. Opens with cursor on first diff line in normal mode
///   2. User visual-selects range, presses <CR>
///   3. Range is recorded on separator line, cursor moves to comment area
///   4. User writes comment, then :wq (or q)
fn generate_comment_lua_script() -> String {
    r##"
local buf = vim.api.nvim_get_current_buf()

-- nvimw starts in insert mode; switch to normal for diff browsing
vim.cmd('stopinsert')

local function find_separator()
  for i = 1, vim.api.nvim_buf_line_count(buf) do
    local line = vim.api.nvim_buf_get_lines(buf, i - 1, i, false)[1]
    if line:match('^========') then
      return i
    end
  end
  return nil
end

local sep_lnum = find_separator()
if not sep_lnum then return end

local diff_start = sep_lnum + 1

-- Position cursor at first diff line
vim.fn.cursor(diff_start, 1)

-- Syntax highlighting for diff lines
vim.cmd([[
  syntax match FzfwDiffAdd /^.\{10\}+.*$/
  syntax match FzfwDiffDel /^.\{10\}-.*$/
  syntax match FzfwDiffSep /^=\+.*$/
  highlight FzfwDiffAdd ctermfg=green guifg=#50fa7b
  highlight FzfwDiffDel ctermfg=red guifg=#ff5555
  highlight FzfwDiffSep ctermfg=gray guifg=#6272a4
  highlight FzfwDiffSelected ctermbg=237 guibg=#44475a
]])

-- Namespace for selected-range highlights
local ns = vim.api.nvim_create_namespace('fzfw_diff_selection')

-- Visual mode <CR>: confirm range selection and jump to comment area
vim.keymap.set('x', '<CR>', function()
  local s = vim.fn.line('v')
  local e = vim.fn.line('.')
  if s > e then s, e = e, s end

  -- Re-find separator (may have shifted if user edited above)
  local cur_sep = find_separator()
  if not cur_sep then return end
  local cur_diff_start = cur_sep + 1

  -- Clamp to diff area
  s = math.max(s, cur_diff_start)
  e = math.max(e, cur_diff_start)

  -- Convert to diff-relative indices (1-based)
  local ds = s - cur_diff_start + 1
  local de = e - cur_diff_start + 1

  -- Update separator line with range info
  local sep_text = string.rep('=', 40) .. 'RANGE:' .. ds .. '-' .. de
  vim.api.nvim_buf_set_lines(buf, cur_sep - 1, cur_sep, false, { sep_text })

  -- Highlight selected range (clear previous selection first)
  vim.api.nvim_buf_clear_namespace(buf, ns, 0, -1)
  for lnum = s, e do
    vim.api.nvim_buf_set_extmark(buf, ns, lnum - 1, 0, {
      line_hl_group = 'FzfwDiffSelected',
    })
  end

  -- Exit visual mode explicitly
  local esc = vim.api.nvim_replace_termcodes('<Esc>', true, false, true)
  vim.api.nvim_feedkeys(esc, 'nx', false)

  -- Move cursor to comment area
  vim.schedule(function()
    vim.fn.cursor(1, 1)
    vim.cmd('startinsert')
  end)
end, { buffer = buf, noremap = true })
"##
    .to_string()
}

/// Parse the nvim comment buffer after editing.
/// Returns (body, range_start, range_end) where range values are 1-based
/// indices into the diff lines section.
/// Returns None if no range was selected.
fn parse_nvim_comment_buffer(content: &str) -> Option<(String, usize, usize)> {
    // Find separator line (starts with "========" and contains "RANGE:")
    let lines: Vec<&str> = content.lines().collect();
    let sep_idx = lines.iter().position(|l| l.starts_with("========"))?;
    let sep_line = lines[sep_idx];

    // Extract range from separator line
    let range_part = sep_line.split("RANGE:").nth(1)?;
    let mut parts = range_part.split('-');
    let start: usize = parts.next()?.parse().ok()?;
    let end: usize = parts.next()?.parse().ok()?;

    if start == 0 || end == 0 || start > end {
        return None;
    }

    // Extract comment body (everything before separator, trimmed)
    let body = lines[..sep_idx].join("\n").trim().to_string();

    Some((body, start, end))
}

async fn add_pending_file_comment_flow(mode: &PrDiff, file_path: &str) -> Result<()> {
    let marker = "=".repeat(40);
    let template = format!("\n{marker}\n{file_path}");

    let tmp_file = tempfile::Builder::new().suffix(".md").tempfile()?;
    std::fs::write(tmp_file.path(), &template)?;

    Command::new("nvimw")
        .arg("--tmux-popup")
        .arg(tmp_file.path())
        .spawn()?
        .wait()
        .await?;

    let content = std::fs::read_to_string(tmp_file.path())?;
    let body = content
        .split(&marker)
        .next()
        .unwrap_or("")
        .trim()
        .to_string();

    if body.is_empty() {
        return Ok(());
    }

    let comment = PendingComment {
        path: file_path.to_string(),
        body,
        line: None,
        side: None,
        start_line: None,
        start_side: None,
        is_file_comment: true,
    };
    mode.pending_comments
        .with_mut(|v| v.comments.push(comment))
        .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Submit review flow
// ---------------------------------------------------------------------------

async fn submit_review_flow(mode: &PrDiff, env: &Env, meta: &PrMeta) -> Result<()> {
    let persisted = mode.pending_comments.get().await?;
    let pending = &persisted.comments;
    if pending.is_empty() {
        env.nvim
            .notify_info("No pending comments to submit")
            .await?;
        return Ok(());
    }

    // 1. Select review event
    let event = fzf::select(vec!["COMMENT", "APPROVE", "REQUEST_CHANGES"]).await?;
    if event.is_empty() {
        return Ok(());
    }

    // 2. Optional review body via nvim popup
    let marker = "=".repeat(40);
    let template = format!("\n{marker}\nReview: {} ({} comments)", event, pending.len());

    let tmp_file = tempfile::Builder::new().suffix(".md").tempfile()?;
    std::fs::write(tmp_file.path(), &template)?;

    Command::new("nvimw")
        .arg("--tmux-popup")
        .arg(tmp_file.path())
        .spawn()?
        .wait()
        .await?;

    let content = std::fs::read_to_string(tmp_file.path())?;
    let review_body = content
        .split(&marker)
        .next()
        .unwrap_or("")
        .trim()
        .to_string();

    // 3. Build review comments (line comments only; file comments handled separately)
    let (line_comments, file_comments): (Vec<_>, Vec<_>) =
        pending.iter().partition(|c| !c.is_file_comment);

    let api_comments: Vec<serde_json::Value> = line_comments
        .iter()
        .map(|c| {
            let mut obj = serde_json::json!({
                "path": c.path,
                "body": c.body,
            });
            if let Some(line) = c.line {
                obj["line"] = serde_json::json!(line);
            }
            if let Some(ref side) = c.side {
                obj["side"] = serde_json::json!(side);
            }
            if let Some(start_line) = c.start_line {
                obj["start_line"] = serde_json::json!(start_line);
            }
            if let Some(ref start_side) = c.start_side {
                obj["start_side"] = serde_json::json!(start_side);
            }
            obj
        })
        .collect();

    // 4. POST review via gh api --input
    let review_payload = serde_json::json!({
        "commit_id": meta.head_sha,
        "event": event,
        "body": review_body,
        "comments": api_comments,
    });

    let payload_file = tempfile::Builder::new().suffix(".json").tempfile()?;
    std::fs::write(payload_file.path(), review_payload.to_string())?;

    let endpoint = format!(
        "repos/{}/{}/pulls/{}/reviews",
        meta.owner, meta.repo, meta.number
    );

    let output = Command::new("gh")
        .args([
            "api",
            &endpoint,
            "--method",
            "POST",
            "--input",
            &payload_file.path().to_string_lossy(),
        ])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("gh api review submit failed: {}", stderr));
    }

    // 5. Post file comments individually (reviews API doesn't support subject_type=file)
    for fc in &file_comments {
        let fc_endpoint = format!(
            "repos/{}/{}/pulls/{}/comments",
            meta.owner, meta.repo, meta.number
        );
        let fc_output = Command::new("gh")
            .args([
                "api",
                &fc_endpoint,
                "-f",
                &format!("body={}", fc.body),
                "-f",
                &format!("commit_id={}", meta.head_sha),
                "-f",
                &format!("path={}", fc.path),
                "-f",
                "subject_type=file",
            ])
            .output()
            .await?;
        if !fc_output.status.success() {
            let stderr = String::from_utf8_lossy(&fc_output.stderr);
            return Err(anyhow!("gh api file comment failed: {}", stderr));
        }
    }

    // 6. Clear pending and delete persistence file
    mode.pending_comments
        .set(PersistedPendingComments {
            head_sha: meta.head_sha.clone(),
            comments: vec![],
        })
        .await;
    mode.pending_comments.delete_file().await;
    env.nvim
        .notify_info(format!("Review submitted ({event})"))
        .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Show pending comments
// ---------------------------------------------------------------------------

async fn show_pending_comments(mode: &PrDiff) -> Result<()> {
    let persisted = mode.pending_comments.get().await?;
    let pending = persisted.comments;
    if pending.is_empty() {
        return Ok(());
    }

    let items: Vec<String> = pending
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let loc = if c.is_file_comment {
                format!("{} (file)", c.path)
            } else if let Some(line) = c.line {
                let side = c.side.as_deref().unwrap_or("RIGHT");
                if let Some(start) = c.start_line {
                    format!("{}:{}-{} ({})", c.path, start, line, side)
                } else {
                    format!("{}:{} ({})", c.path, line, side)
                }
            } else {
                c.path.clone()
            };
            let body_preview: String = c.body.chars().take(60).collect();
            let body_preview = body_preview.replace('\n', " ");
            format!("[{}] {} | {}", i + 1, loc, body_preview)
        })
        .collect();

    let refs: Vec<&str> = items.iter().map(|s| s.as_str()).collect();
    // Read-only display; ignore selection
    let _ = fzf::select(refs).await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_name_is_pr_diff() {
        assert_eq!(PrDiff::new().name(), "pr-diff");
    }

    #[test]
    fn parse_hunk_header_basic() {
        let (new_start, new_lines, old_start) =
            parse_hunk_header("@@ -10,5 +20,7 @@ fn foo()").unwrap();
        assert_eq!(old_start, 10);
        assert_eq!(new_start, 20);
        assert_eq!(new_lines, 7);
    }

    #[test]
    fn parse_hunk_header_single_line() {
        let (new_start, new_lines, old_start) = parse_hunk_header("@@ -1 +1 @@").unwrap();
        assert_eq!(old_start, 1);
        assert_eq!(new_start, 1);
        assert_eq!(new_lines, 1);
    }

    #[test]
    fn parse_hunk_header_with_comma() {
        let (new_start, new_lines, old_start) = parse_hunk_header("@@ -100,3 +200,10 @@").unwrap();
        assert_eq!(old_start, 100);
        assert_eq!(new_start, 200);
        assert_eq!(new_lines, 10);
    }

    #[test]
    fn parse_unified_diff_single_hunk() {
        let diff = "\
diff --git a/src/main.rs b/src/main.rs
index abc..def 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
 fn main() {
+    println!(\"hello\");
     let x = 1;
 }";
        let hunks = parse_unified_diff(diff);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].file_path, "src/main.rs");
        assert_eq!(hunks[0].new_start, 1);
        assert_eq!(hunks[0].lines.len(), 4);
        assert_eq!(hunks[0].lines[0].kind, DiffLineKind::Context);
        assert_eq!(hunks[0].lines[0].new_lineno, Some(1));
        assert_eq!(hunks[0].lines[0].old_lineno, Some(1));
        assert_eq!(hunks[0].lines[1].kind, DiffLineKind::Added);
        assert_eq!(hunks[0].lines[1].new_lineno, Some(2));
        assert_eq!(hunks[0].lines[1].old_lineno, None);
    }

    #[test]
    fn parse_unified_diff_multiple_files() {
        let diff = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,2 +1,3 @@
 line1
+added
 line2
diff --git a/b.rs b/b.rs
--- a/b.rs
+++ b/b.rs
@@ -5,3 +5,2 @@
 keep
-removed
 keep2";
        let hunks = parse_unified_diff(diff);
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].file_path, "a.rs");
        assert_eq!(hunks[1].file_path, "b.rs");
        assert_eq!(hunks[1].lines[1].kind, DiffLineKind::Removed);
        assert_eq!(hunks[1].lines[1].old_lineno, Some(6));
    }

    #[test]
    fn parse_unified_diff_multiple_hunks_same_file() {
        let diff = "\
diff --git a/f.rs b/f.rs
--- a/f.rs
+++ b/f.rs
@@ -1,2 +1,3 @@
 a
+b
 c
@@ -10,2 +11,3 @@
 x
+y
 z";
        let hunks = parse_unified_diff(diff);
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].new_start, 1);
        assert_eq!(hunks[1].new_start, 11);
    }

    #[test]
    fn render_parse_hunk_item_roundtrip() {
        let hunk = DiffHunk {
            file_path: "src/lib.rs".to_string(),
            hunk_header: "@@ -1,3 +1,5 @@".to_string(),
            new_start: 1,
            lines: vec![
                DiffLine {
                    kind: DiffLineKind::Added,
                    content: "new".to_string(),
                    old_lineno: None,
                    new_lineno: Some(1),
                },
                DiffLine {
                    kind: DiffLineKind::Added,
                    content: "new2".to_string(),
                    old_lineno: None,
                    new_lineno: Some(2),
                },
                DiffLine {
                    kind: DiffLineKind::Removed,
                    content: "old".to_string(),
                    old_lineno: Some(1),
                    new_lineno: None,
                },
            ],
            raw_text: String::new(),
            rename_from: None,
        };
        let rendered = render_hunk_item(42, &hunk);
        assert!(rendered.contains("src/lib.rs:1"));
        assert!(rendered.contains("(+2/-1)"));
        let idx = parse_hunk_index(&rendered).unwrap();
        assert_eq!(idx, 42);
    }

    #[test]
    fn colorize_hunk_adds_ansi() {
        let hunk = DiffHunk {
            file_path: "f.rs".to_string(),
            hunk_header: "@@ -1,1 +1,2 @@".to_string(),
            new_start: 1,
            lines: vec![],
            raw_text: "@@ -1,1 +1,2 @@\n keep\n+added".to_string(),
            rename_from: None,
        };
        let colored = colorize_hunk(&hunk);
        // Should contain ANSI escape codes
        assert!(colored.contains("\x1b["));
    }

    #[test]
    fn parse_unified_diff_skips_binary() {
        let diff = "\
diff --git a/image.png b/image.png
Binary files /dev/null and b/image.png differ
diff --git a/code.rs b/code.rs
--- a/code.rs
+++ b/code.rs
@@ -1,1 +1,2 @@
 existing
+new";
        let hunks = parse_unified_diff(diff);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].file_path, "code.rs");
    }

    #[test]
    fn parse_unified_diff_deleted_file() {
        let diff = "\
diff --git a/old.rs b/old.rs
--- a/old.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-line1
-line2";
        let hunks = parse_unified_diff(diff);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].file_path, "/dev/null");
    }

    #[test]
    fn wants_sort_is_false() {
        assert!(!PrDiff::new().wants_sort());
    }

    // -- rename tests --

    #[test]
    fn parse_unified_diff_rename_only() {
        let diff = "\
diff --git a/old.rs b/new.rs
similarity index 100%
rename from old.rs
rename to new.rs";
        let hunks = parse_unified_diff(diff);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].file_path, "new.rs");
        assert_eq!(hunks[0].rename_from.as_deref(), Some("old.rs"));
        assert!(hunks[0].lines.is_empty());
        assert!(hunks[0].raw_text.contains("rename from old.rs"));
    }

    #[test]
    fn parse_unified_diff_rename_with_changes() {
        let diff = "\
diff --git a/old.rs b/new.rs
similarity index 90%
rename from old.rs
rename to new.rs
--- a/old.rs
+++ b/new.rs
@@ -1,2 +1,3 @@
 keep
+added
 keep2";
        let hunks = parse_unified_diff(diff);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].file_path, "new.rs");
        assert_eq!(hunks[0].rename_from.as_deref(), Some("old.rs"));
        assert_eq!(hunks[0].lines.len(), 3);
    }

    #[test]
    fn render_hunk_item_rename_only() {
        let hunk = DiffHunk {
            file_path: "new.rs".to_string(),
            hunk_header: String::new(),
            new_start: 0,
            lines: vec![],
            raw_text: String::new(),
            rename_from: Some("old.rs".to_string()),
        };
        let rendered = render_hunk_item(5, &hunk);
        assert!(rendered.contains("old.rs → new.rs"));
        assert!(rendered.contains("(renamed)"));
        assert_eq!(parse_hunk_index(&rendered).unwrap(), 5);
    }

    #[test]
    fn render_hunk_item_rename_with_changes() {
        let hunk = DiffHunk {
            file_path: "new.rs".to_string(),
            hunk_header: "@@ -1,2 +1,3 @@".to_string(),
            new_start: 1,
            lines: vec![DiffLine {
                kind: DiffLineKind::Added,
                content: "x".to_string(),
                old_lineno: None,
                new_lineno: Some(1),
            }],
            raw_text: String::new(),
            rename_from: Some("old.rs".to_string()),
        };
        let rendered = render_hunk_item(3, &hunk);
        assert!(rendered.contains("old.rs → new.rs:1"));
        assert!(rendered.contains("(+1/-0)"));
    }

    #[test]
    fn parse_unified_diff_rename_followed_by_normal() {
        let diff = "\
diff --git a/old.rs b/new.rs
similarity index 100%
rename from old.rs
rename to new.rs
diff --git a/other.rs b/other.rs
--- a/other.rs
+++ b/other.rs
@@ -1,1 +1,2 @@
 keep
+added";
        let hunks = parse_unified_diff(diff);
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].file_path, "new.rs");
        assert_eq!(hunks[0].rename_from.as_deref(), Some("old.rs"));
        assert!(hunks[0].lines.is_empty());
        assert_eq!(hunks[1].file_path, "other.rs");
        assert!(hunks[1].rename_from.is_none());
    }

    // -- compute_comment_range tests --

    fn make_lines_for_range_tests() -> Vec<DiffLine> {
        // Simulates a hunk like:
        //  context1       old=10, new=20
        // -removed1       old=11
        // -removed2       old=12
        // +added1                 new=21
        // +added2                 new=22
        //  context2       old=13, new=23
        vec![
            DiffLine {
                kind: DiffLineKind::Context,
                content: "context1".into(),
                old_lineno: Some(10),
                new_lineno: Some(20),
            },
            DiffLine {
                kind: DiffLineKind::Removed,
                content: "removed1".into(),
                old_lineno: Some(11),
                new_lineno: None,
            },
            DiffLine {
                kind: DiffLineKind::Removed,
                content: "removed2".into(),
                old_lineno: Some(12),
                new_lineno: None,
            },
            DiffLine {
                kind: DiffLineKind::Added,
                content: "added1".into(),
                old_lineno: None,
                new_lineno: Some(21),
            },
            DiffLine {
                kind: DiffLineKind::Added,
                content: "added2".into(),
                old_lineno: None,
                new_lineno: Some(22),
            },
            DiffLine {
                kind: DiffLineKind::Context,
                content: "context2".into(),
                old_lineno: Some(13),
                new_lineno: Some(23),
            },
        ]
    }

    #[test]
    fn range_all_added_uses_right() {
        let lines = make_lines_for_range_tests();
        let (side, start, end) = compute_comment_range(&lines, &[3, 4]).unwrap();
        assert_eq!(side, "RIGHT");
        assert_eq!(start, 21);
        assert_eq!(end, 22);
    }

    #[test]
    fn range_all_removed_uses_left() {
        let lines = make_lines_for_range_tests();
        let (side, start, end) = compute_comment_range(&lines, &[1, 2]).unwrap();
        assert_eq!(side, "LEFT");
        assert_eq!(start, 11);
        assert_eq!(end, 12);
    }

    #[test]
    fn range_mixed_removed_and_added_expands_to_right() {
        let lines = make_lines_for_range_tests();
        // Select removed1 (idx=1) and added1 (idx=3)
        let (side, start, end) = compute_comment_range(&lines, &[1, 3]).unwrap();
        assert_eq!(side, "RIGHT");
        // start: search backward from idx=1, finds new_lineno=20 at idx=0
        assert_eq!(start, 20);
        // end: search forward from idx=3, finds new_lineno=21 at idx=3
        assert_eq!(end, 21);
    }

    #[test]
    fn range_removed_only_at_start_expands_to_right_with_context() {
        let lines = make_lines_for_range_tests();
        // Select removed1 (idx=1) and context2 (idx=5)
        let (side, start, end) = compute_comment_range(&lines, &[1, 5]).unwrap();
        assert_eq!(side, "RIGHT");
        // start: search backward from idx=1, finds new_lineno=20 at idx=0
        assert_eq!(start, 20);
        // end: idx=5 has new_lineno=23
        assert_eq!(end, 23);
    }

    #[test]
    fn range_single_context_line() {
        let lines = make_lines_for_range_tests();
        let (side, start, end) = compute_comment_range(&lines, &[0]).unwrap();
        assert_eq!(side, "RIGHT");
        assert_eq!(start, 20);
        assert_eq!(end, 20);
    }

    #[test]
    fn range_single_removed_uses_left() {
        let lines = make_lines_for_range_tests();
        let (side, start, end) = compute_comment_range(&lines, &[1]).unwrap();
        assert_eq!(side, "LEFT");
        assert_eq!(start, 11);
        assert_eq!(end, 11);
    }

    #[test]
    fn range_all_lines_selected() {
        let lines = make_lines_for_range_tests();
        let (side, start, end) = compute_comment_range(&lines, &[0, 1, 2, 3, 4, 5]).unwrap();
        assert_eq!(side, "RIGHT");
        assert_eq!(start, 20);
        assert_eq!(end, 23);
    }

    #[test]
    fn range_removed_at_end_expands_forward() {
        // Hunk where removed lines are at the end with no context after
        let lines = vec![
            DiffLine {
                kind: DiffLineKind::Context,
                content: "ctx".into(),
                old_lineno: Some(1),
                new_lineno: Some(1),
            },
            DiffLine {
                kind: DiffLineKind::Added,
                content: "new".into(),
                old_lineno: None,
                new_lineno: Some(2),
            },
            DiffLine {
                kind: DiffLineKind::Removed,
                content: "old".into(),
                old_lineno: Some(2),
                new_lineno: None,
            },
        ];
        // Select added + removed (mixed)
        let (side, start, end) = compute_comment_range(&lines, &[1, 2]).unwrap();
        assert_eq!(side, "RIGHT");
        assert_eq!(start, 2);
        // end: search forward from idx=2 finds nothing, search backward finds new_lineno=2 at idx=1
        assert_eq!(end, 2);
    }

    // -- PendingComment serialize/deserialize tests --

    #[test]
    fn pending_comment_serde_roundtrip() {
        let comment = PendingComment {
            path: "src/main.rs".to_string(),
            body: "looks good".to_string(),
            line: Some(42),
            side: Some("RIGHT".to_string()),
            start_line: Some(40),
            start_side: Some("RIGHT".to_string()),
            is_file_comment: false,
        };
        let json = serde_json::to_string(&comment).unwrap();
        let restored: PendingComment = serde_json::from_str(&json).unwrap();
        assert_eq!(comment, restored);
    }

    #[test]
    fn pending_comment_file_comment_roundtrip() {
        let comment = PendingComment {
            path: "README.md".to_string(),
            body: "update docs".to_string(),
            line: None,
            side: None,
            start_line: None,
            start_side: None,
            is_file_comment: true,
        };
        let json = serde_json::to_string(&comment).unwrap();
        let restored: PendingComment = serde_json::from_str(&json).unwrap();
        assert_eq!(comment, restored);
    }

    #[test]
    fn persisted_pending_comments_roundtrip() {
        let persisted = PersistedPendingComments {
            head_sha: "abc123".to_string(),
            comments: vec![PendingComment {
                path: "lib.rs".to_string(),
                body: "fix this".to_string(),
                line: Some(10),
                side: Some("LEFT".to_string()),
                start_line: None,
                start_side: None,
                is_file_comment: false,
            }],
        };
        let json = serde_json::to_string(&persisted).unwrap();
        let restored: PersistedPendingComments = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.head_sha, "abc123");
        assert_eq!(restored.comments.len(), 1);
        assert_eq!(restored.comments[0], persisted.comments[0]);
    }

    #[test]
    fn stale_detection_same_sha() {
        let persisted = PersistedPendingComments {
            head_sha: "abc123".to_string(),
            comments: vec![],
        };
        let current_sha = "abc123";
        assert_eq!(persisted.head_sha, current_sha);
    }

    #[test]
    fn stale_detection_different_sha() {
        let persisted = PersistedPendingComments {
            head_sha: "abc123".to_string(),
            comments: vec![],
        };
        let current_sha = "def456";
        assert_ne!(persisted.head_sha, current_sha);
    }

    // -- format_diff_lines_for_buffer tests --

    #[test]
    fn format_diff_lines_basic() {
        let lines = make_lines_for_range_tests();
        let formatted = format_diff_lines_for_buffer(&lines);
        assert_eq!(formatted.len(), 6);
        assert_eq!(formatted[0], "  10   20  context1");
        assert_eq!(formatted[1], "  11      -removed1");
        assert_eq!(formatted[2], "  12      -removed2");
        assert_eq!(formatted[3], "       21 +added1");
        assert_eq!(formatted[4], "       22 +added2");
        assert_eq!(formatted[5], "  13   23  context2");
    }

    // -- parse_nvim_comment_buffer tests --

    #[test]
    fn parse_nvim_comment_buffer_with_comment_and_range() {
        let content = "\
This is my comment.
It spans multiple lines.
========================================RANGE:2-4
  10   20  context1
       21 +added1
       22 +added2
  11      -removed1
  13   23  context2";
        let (body, start, end) = parse_nvim_comment_buffer(content).unwrap();
        assert_eq!(body, "This is my comment.\nIt spans multiple lines.");
        assert_eq!(start, 2);
        assert_eq!(end, 4);
    }

    #[test]
    fn parse_nvim_comment_buffer_no_range() {
        let content = "\
\n========================================\n  10   20  context1";
        // No RANGE in separator → None
        assert!(parse_nvim_comment_buffer(content).is_none());
    }

    #[test]
    fn parse_nvim_comment_buffer_empty_comment() {
        let content = "\
\n========================================RANGE:1-3\n  10   20  context1";
        let (body, start, end) = parse_nvim_comment_buffer(content).unwrap();
        assert_eq!(body, "");
        assert_eq!(start, 1);
        assert_eq!(end, 3);
    }

    #[test]
    fn parse_nvim_comment_buffer_single_line_range() {
        let content = "\
Fix this bug
========================================RANGE:3-3
  10   20  context1
       21 +added1
       22 +added2";
        let (body, start, end) = parse_nvim_comment_buffer(content).unwrap();
        assert_eq!(body, "Fix this bug");
        assert_eq!(start, 3);
        assert_eq!(end, 3);
    }

    #[test]
    fn parse_nvim_comment_buffer_invalid_range() {
        // start > end
        let content = "comment\n========================================RANGE:5-2\ndiff";
        assert!(parse_nvim_comment_buffer(content).is_none());
    }
}
