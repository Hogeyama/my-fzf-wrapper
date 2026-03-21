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
use crate::mode::ModeDef;
use crate::state::State;
use crate::utils::fzf;
use crate::utils::fzf::PreviewWindow;
use crate::utils::git;
use crate::utils::xsel;

#[derive(Clone)]
pub struct PrDiff {
    hunks: ModeCache<Vec<DiffHunk>>,
    pr_meta: ModeCache<PrMeta>,
}

impl PrDiff {
    pub fn new() -> Self {
        Self {
            hunks: ModeCache::new(),
            pr_meta: ModeCache::new(),
        }
    }
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

    fn load<'a>(
        &'a self,
        _env: &'a Env,
        _state: &'a mut State,
        _query: String,
        _item: String,
    ) -> super::LoadStream<'a> {
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
                .with(|hunks| {
                    hunks
                        .get(idx)
                        .map(colorize_hunk)
                })
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
                execute!(b, |_mode, env, _state, _query, item| {
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
                execute_silent!(b, |_mode, _env, _state, _query, item| {
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
                select_and_execute!{b, |mode, _env, _state, _query, item|
                    "comment" => {
                        let idx = parse_hunk_index(&item)?;
                        let hunk = mode
                            .hunks
                            .with(|hunks| hunks.get(idx).cloned())
                            .await
                            .ok()
                            .flatten()
                            .ok_or_else(|| anyhow!("hunk not found"))?;
                        let meta = mode.pr_meta.get().await?;
                        post_comment_flow(&meta, &hunk).await
                    },
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
                        Ok(())
                    },
                }
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
    let output = Command::new("gh")
        .args(["pr", "diff"])
        .output()
        .await?;
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

    let lines: Vec<&str> = diff.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        if line.starts_with("diff --git ") {
            current_file = None;
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
                });
                continue;
            }
        }

        // Binary files, index lines, etc.
        i += 1;
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
        "{}:{} (+{}/-{}) |{}",
        hunk.file_path, hunk.new_start, additions, deletions, index
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
// Comment flow
// ---------------------------------------------------------------------------

async fn post_comment_flow(meta: &PrMeta, hunk: &DiffHunk) -> Result<()> {
    // 1. Show hunk lines in fzf multi-select for line range selection
    let line_items: Vec<String> = hunk
        .lines
        .iter()
        .map(|l| {
            let lineno = match l.kind {
                DiffLineKind::Removed => l
                    .old_lineno
                    .map(|n| format!("{:>4}", n))
                    .unwrap_or_else(|| "    ".to_string()),
                _ => l
                    .new_lineno
                    .map(|n| format!("{:>4}", n))
                    .unwrap_or_else(|| "    ".to_string()),
            };
            let prefix = match l.kind {
                DiffLineKind::Added => "+",
                DiffLineKind::Removed => "-",
                DiffLineKind::Context => " ",
            };
            format!("{} {}{}", lineno, prefix, l.content)
        })
        .collect();

    let line_refs: Vec<&str> = line_items.iter().map(|s| s.as_str()).collect();
    let selected = fzf::select_multi(line_refs).await?;

    if selected.is_empty() {
        return Ok(());
    }

    // 2. Determine line range and side
    let selected_indices: Vec<usize> = selected
        .iter()
        .filter_map(|sel| {
            hunk.lines.iter().position(|l| {
                let lineno = match l.kind {
                    DiffLineKind::Removed => l.old_lineno,
                    _ => l.new_lineno,
                };
                if let Some(n) = lineno {
                    sel.starts_with(&format!("{:>4}", n))
                } else {
                    false
                }
            })
        })
        .collect();

    if selected_indices.is_empty() {
        return Ok(());
    }

    let selected_lines: Vec<&DiffLine> = selected_indices
        .iter()
        .map(|&i| &hunk.lines[i])
        .collect();

    let all_removed = selected_lines
        .iter()
        .all(|l| l.kind == DiffLineKind::Removed);

    let (side, start_line, end_line) = if all_removed {
        let line_numbers: Vec<usize> = selected_lines
            .iter()
            .filter_map(|l| l.old_lineno)
            .collect();
        let start = *line_numbers.iter().min().unwrap();
        let end = *line_numbers.iter().max().unwrap();
        ("LEFT", start, end)
    } else {
        let line_numbers: Vec<usize> = selected_lines
            .iter()
            .filter_map(|l| l.new_lineno)
            .collect();
        if line_numbers.is_empty() {
            return Err(anyhow!("no valid line numbers in selection"));
        }
        let start = *line_numbers.iter().min().unwrap();
        let end = *line_numbers.iter().max().unwrap();
        ("RIGHT", start, end)
    };

    // 3. Compose comment body via nvim popup
    let selected_code: String = selected_lines
        .iter()
        .map(|l| {
            let prefix = match l.kind {
                DiffLineKind::Added => "+",
                DiffLineKind::Removed => "-",
                DiffLineKind::Context => " ",
            };
            format!("{}{}", prefix, l.content)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let marker = "=".repeat(40);
    let template = format!(
        "\n{marker}\n{}:{}-{} ({})\n```\n{}\n```",
        hunk.file_path, start_line, end_line, side, selected_code
    );

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

    // 4. Post the comment via gh api
    let endpoint = format!(
        "repos/{}/{}/pulls/{}/comments",
        meta.owner, meta.repo, meta.number
    );

    let mut args = vec![
        "api".to_string(),
        endpoint,
        "-f".to_string(),
        format!("body={}", body),
        "-f".to_string(),
        format!("commit_id={}", meta.head_sha),
        "-f".to_string(),
        format!("path={}", hunk.file_path),
        "-f".to_string(),
        format!("side={}", side),
        "-F".to_string(),
        format!("line={}", end_line),
    ];

    if start_line != end_line {
        args.push("-F".to_string());
        args.push(format!("start_line={}", start_line));
        args.push("-f".to_string());
        args.push(format!("start_side={}", side));
    }

    let output = Command::new("gh").args(&args).output().await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("gh api comment failed: {}", stderr));
    }

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
        let (new_start, new_lines, old_start) =
            parse_hunk_header("@@ -1 +1 @@").unwrap();
        assert_eq!(old_start, 1);
        assert_eq!(new_start, 1);
        assert_eq!(new_lines, 1);
    }

    #[test]
    fn parse_hunk_header_with_comma() {
        let (new_start, new_lines, old_start) =
            parse_hunk_header("@@ -100,3 +200,10 @@").unwrap();
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
}
