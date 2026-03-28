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
use crate::utils::bat;
use crate::utils::browser;
use crate::utils::fzf;
use crate::utils::fzf::PreviewWindow;
use crate::utils::git;
use crate::utils::xsel;

#[derive(Clone)]
pub struct GitReview {
    threads: ModeCache<Vec<ReviewThreadItem>>,
    unresolved_only: ModeCache<bool>,
}

impl GitReview {
    pub fn new() -> Self {
        Self {
            threads: ModeCache::new(),
            unresolved_only: ModeCache::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReviewThreadItem {
    id: String,
    url: String,
    path: String,
    line_start: usize,
    line_end: usize,
    revision: String,
    is_resolved: bool,
    author: String,
    summary: String,
    body: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhPrNumber {
    number: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQlResp {
    data: GraphQlData,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQlData {
    repository: Option<RepositoryData>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryData {
    pull_request: Option<PullRequestData>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullRequestData {
    review_threads: ReviewThreadConnection,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewThreadConnection {
    nodes: Vec<ReviewThreadNode>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewThreadNode {
    id: String,
    is_resolved: bool,
    path: String,
    start_line: Option<i64>,
    original_start_line: Option<i64>,
    line: Option<i64>,
    original_line: Option<i64>,
    comments: ReviewCommentConnection,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewCommentConnection {
    nodes: Vec<ReviewCommentNode>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewCommentNode {
    url: String,
    body: String,
    author: Option<ReviewCommentAuthor>,
    commit: Option<CommitRef>,
    original_commit: Option<CommitRef>,
}

#[derive(Deserialize)]
struct ReviewCommentAuthor {
    login: String,
}

#[derive(Deserialize)]
struct CommitRef {
    oid: String,
}

impl ModeDef for GitReview {
    fn name(&self) -> &'static str {
        "pr-threads"
    }

    fn load<'a>(&'a self, _env: &'a Env, _query: String, _item: String) -> super::LoadStream<'a> {
        Box::pin(async_stream::stream! {
            let threads = fetch_review_threads().await?;
            let unresolved_only = self.unresolved_only.get().await.unwrap_or(false);
            let items = threads.iter()
                .filter(|t| !unresolved_only || !t.is_resolved)
                .map(render_thread_item)
                .collect::<Vec<_>>();
            self.threads.set(threads).await;
            yield Ok(LoadResp::new_with_default_header(items));
        })
    }

    fn preview<'a>(
        &'a self,
        _env: &Env,
        win: &PreviewWindow,
        item: String,
    ) -> BoxFuture<'a, Result<PreviewResp>> {
        let win = *win;
        async move {
            let parsed = parse_thread_item(&item)?;
            let thread = self
                .threads
                .with(|threads| threads.iter().find(|t| t.id == parsed.id).cloned())
                .await
                .ok()
                .flatten()
                .unwrap_or(parsed);

            let git_ref = format!("{}:{}", thread.revision, thread.path);
            let git_output = Command::new("git")
                .args(["show", &git_ref])
                .output()
                .await?;

            let code_lines = win.lines / 2;
            let code = if git_output.status.success() {
                bat::render_stdin_with_highlight_range(
                    &git_output.stdout,
                    &thread.path,
                    thread.line_start as isize,
                    thread.line_end as isize,
                    code_lines,
                    win.columns,
                )
                .await
                .unwrap_or_else(|_| "(code preview unavailable)".to_string())
            } else {
                format!("(git show {} failed)", git_ref)
            };

            let message = compose_preview_text(&thread, code, win.columns).await;
            Ok(PreviewResp { message })
        }
        .boxed()
    }

    fn fzf_bindings(&self) -> (super::ModeBindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [
                execute!{b, |_mode,env,_query,item| {
                    let thread = parse_thread_item(&item)?;
                    let workdir = git::workdir()?;
                    let file = format!("{}{}", workdir, thread.path);
                    actions::open_in_nvim(env, file, Some(thread.line_start), false).await
                }}
            ],
            "ctrl-y" => [
                execute!(b, |mode, _env, _query, item| {
                    let parsed = parse_thread_item(&item)?;
                    let thread = mode.threads.with(|threads| {
                        threads.iter().find(|t| t.id == parsed.id).cloned()
                    }).await
                    .ok()
                    .flatten()
                    .unwrap_or(parsed);
                    let text = compose_yank_text(&thread).await?;
                    xsel::yank(&text).await?;
                    Ok(())
                })
            ],
            "pgup" => [
                execute!(b, |mode, _env, _query, item| {
                    let parsed = parse_thread_item(&item)?;
                    let resolve_label = if parsed.is_resolved {
                        "unresolve"
                    } else {
                        "resolve"
                    };
                    match &*fzf::select(vec!["browse", "reply", resolve_label]).await? {
                        "reply" => {
                            let thread = mode.threads.with(|threads| {
                                threads.iter().find(|t| t.id == parsed.id).cloned()
                            }).await
                            .ok()
                            .flatten()
                            .unwrap_or(parsed);
                            reply_to_thread(&thread).await
                        }
                        "resolve" => {
                            resolve_thread(&parsed.id).await
                        }
                        "unresolve" => {
                            unresolve_thread(&parsed.id).await
                        }
                        "browse" => {
                            browser::open(&parsed.url).await
                        }
                        _ => Ok(()),
                    }
                }),
                b.reload()
            ],
            "ctrl-x" => [
                execute_silent!(b, |mode, _env, _query, _item| {
                    let current = mode.unresolved_only.get().await.unwrap_or(false);
                    mode.unresolved_only.set(!current).await;
                    Ok(())
                }),
                b.reload_with_as::<Self, _>(|mode, _env, _query, _item| {
                    let unresolved_only = mode.unresolved_only.clone();
                    let threads = mode.threads.clone();
                    Box::pin(async_stream::stream! {
                        let threads = threads.get().await.unwrap_or_default();
                        let flag = unresolved_only.get().await.unwrap_or(false);
                        let items = threads.iter()
                            .filter(|t| !flag || !t.is_resolved)
                            .map(render_thread_item)
                            .collect::<Vec<_>>();
                        yield Ok(LoadResp::new_with_default_header(items));
                    })
                })
            ],
        }
    }

    fn wants_sort(&self) -> bool {
        false
    }
}

async fn compose_preview_text(thread: &ReviewThreadItem, code: String, columns: usize) -> String {
    let status = if thread.is_resolved {
        "✅ Resolved"
    } else {
        "💬 Unresolved"
    };
    let header = format!(
        "{} {} {}:{}-{}",
        status, thread.author, thread.path, thread.line_start, thread.line_end
    );
    let body = render_comments_with_glow(&thread.body, columns)
        .await
        .unwrap_or_else(|_| thread.body.clone());
    format!(
        "{header}\n{code}\n{body}",
        code = code.trim_end_matches('\n'),
    )
}

async fn render_comments_with_glow(body: &str, columns: usize) -> Result<String> {
    let width = std::cmp::max(40, columns);

    let mut child = Command::new("glow")
        .args(["-s", "dark", "-w", &width.to_string(), "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(body.as_bytes()).await?;
    }

    let output = child.wait_with_output().await?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string())
}

async fn compose_yank_text(thread: &ReviewThreadItem) -> Result<String> {
    let status = if thread.is_resolved {
        "Resolved"
    } else {
        "Unresolved"
    };
    let header = format!(
        "[{}] {} {}:{}-{}",
        status, thread.author, thread.path, thread.line_start, thread.line_end
    );
    let code = render_code_with_line_numbers(thread).await?;
    Ok(format!("{header}\n{code}\n\n{body}", body = thread.body))
}

async fn render_code_with_line_numbers(thread: &ReviewThreadItem) -> Result<String> {
    let git_ref = format!("{}:{}", thread.revision, thread.path);
    let output = Command::new("git")
        .args(["show", &git_ref])
        .output()
        .await?;
    if !output.status.success() {
        return Err(anyhow!("git show {} failed", git_ref));
    }
    let content = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = content.lines().collect();
    let start = thread.line_start.saturating_sub(1);
    let end = std::cmp::min(thread.line_end, lines.len());
    let width = end.to_string().len();
    let numbered: Vec<String> = lines[start..end]
        .iter()
        .enumerate()
        .map(|(i, line)| format!("{:>width$}  {}", start + i + 1, line))
        .collect();
    Ok(numbered.join("\n"))
}

async fn reply_to_thread(thread: &ReviewThreadItem) -> Result<()> {
    let yank_text = compose_yank_text(thread).await?;
    let marker = "=".repeat(40);
    let template = format!("\n{marker}\n{yank_text}");

    let tmp_file = tempfile::Builder::new().suffix(".md").tempfile()?;
    std::fs::write(tmp_file.path(), &template)?;

    Command::new("nvimw")
        .arg("--tmux-popup")
        .arg(tmp_file.path())
        .spawn()?
        .wait()
        .await?;

    let content = std::fs::read_to_string(tmp_file.path())?;
    let reply_body = content
        .split(&marker)
        .next()
        .unwrap_or("")
        .trim()
        .to_string();

    if reply_body.is_empty() {
        return Ok(());
    }

    let mutation = reply_mutation();
    let output = Command::new("gh")
        .args([
            "api",
            "graphql",
            "-f",
            &format!("query={mutation}"),
            "-f",
            &format!("threadId={}", thread.id),
            "-f",
            &format!("body={reply_body}"),
        ])
        .output()
        .await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("reply failed: {}", stderr));
    }
    Ok(())
}

fn reply_mutation() -> &'static str {
    "mutation($threadId:ID!, $body:String!) { addPullRequestReviewThreadReply(input:{pullRequestReviewThreadId:$threadId, body:$body}) { comment { url } } }"
}

async fn resolve_thread(thread_id: &str) -> Result<()> {
    let output = Command::new("gh")
        .args([
            "api",
            "graphql",
            "-f",
            "query=mutation($threadId:ID!) { resolveReviewThread(input:{threadId:$threadId}) { thread { id } } }",
            "-f",
            &format!("threadId={thread_id}"),
        ])
        .output()
        .await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("resolve failed: {}", stderr));
    }
    Ok(())
}

async fn unresolve_thread(thread_id: &str) -> Result<()> {
    let output = Command::new("gh")
        .args([
            "api",
            "graphql",
            "-f",
            "query=mutation($threadId:ID!) { unresolveReviewThread(input:{threadId:$threadId}) { thread { id } } }",
            "-f",
            &format!("threadId={thread_id}"),
        ])
        .output()
        .await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("unresolve failed: {}", stderr));
    }
    Ok(())
}

async fn fetch_review_threads() -> Result<Vec<ReviewThreadItem>> {
    let pr_number = current_pr_number().await?;
    let (owner, repo) = current_repo_owner_name().await?;
    let query = graphql_query();

    let output = Command::new("gh")
        .args([
            "api",
            "graphql",
            "-f",
            &format!("query={query}"),
            "-F",
            &format!("owner={owner}"),
            "-F",
            &format!("repo={repo}"),
            "-F",
            &format!("pr={pr_number}"),
        ])
        .output()
        .await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("gh api graphql failed: {}", stderr));
    }

    let resp: GraphQlResp = serde_json::from_slice(&output.stdout)?;
    let Some(repository) = resp.data.repository else {
        return Ok(vec![]);
    };
    let Some(pr) = repository.pull_request else {
        return Ok(vec![]);
    };

    let items = pr
        .review_threads
        .nodes
        .into_iter()
        .filter_map(|t| {
            let first = t.comments.nodes.first()?;
            let (line_start, line_end, revision) = if let Some(line) = t.line {
                let start = t.start_line.unwrap_or(line);
                let rev = first
                    .commit
                    .as_ref()
                    .map(|c| c.oid.clone())
                    .unwrap_or_else(|| "HEAD".to_string());
                (start, line, rev)
            } else if let Some(orig_line) = t.original_line {
                let start = t.original_start_line.unwrap_or(orig_line);
                let rev = first
                    .original_commit
                    .as_ref()
                    .map(|c| c.oid.clone())
                    .unwrap_or_else(|| "HEAD".to_string());
                (start, orig_line, rev)
            } else {
                (1, 1, "HEAD".to_string())
            };
            let line_start = usize::try_from(line_start).ok()?;
            let line_end = usize::try_from(line_end).ok()?;
            let author = first
                .author
                .as_ref()
                .map(|a| a.login.clone())
                .unwrap_or_else(|| "unknown".to_string());
            let summary = summarize_comment(&first.body);
            let body = t
                .comments
                .nodes
                .iter()
                .map(|c| {
                    let a = c
                        .author
                        .as_ref()
                        .map(|x| x.login.as_str())
                        .unwrap_or("unknown");
                    format!("{}:\n{}", a, c.body)
                })
                .collect::<Vec<_>>()
                .join("\n\n---\n\n");
            Some(ReviewThreadItem {
                id: t.id,
                url: first.url.clone(),
                path: t.path,
                line_start: std::cmp::min(line_start, line_end),
                line_end: std::cmp::max(line_start, line_end),
                revision,
                is_resolved: t.is_resolved,
                author,
                summary,
                body,
            })
        })
        .collect::<Vec<_>>();

    Ok(items)
}

async fn current_pr_number() -> Result<u64> {
    let output = Command::new("gh")
        .args(["pr", "view", "--json", "number"])
        .output()
        .await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("gh pr view --json number failed: {}", stderr));
    }
    let pr: GhPrNumber = serde_json::from_slice(&output.stdout)?;
    Ok(pr.number)
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
        return Err(anyhow!("gh repo view --json owner,name failed: {}", stderr));
    }
    let repo: RepoView = serde_json::from_slice(&output.stdout)?;
    Ok((repo.owner.login, repo.name))
}

fn graphql_query() -> &'static str {
    "query($owner:String!, $repo:String!, $pr:Int!) { repository(owner:$owner, name:$repo) { pullRequest(number:$pr) { reviewThreads(first:100) { nodes { id isResolved path line originalLine startLine originalStartLine comments(first:30) { nodes { url body author { login } commit { oid } originalCommit { oid } } } } } } } }"
}

fn summarize_comment(body: &str) -> String {
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("(empty comment)")
        .chars()
        .take(120)
        .collect()
}

fn render_thread_item(item: &ReviewThreadItem) -> String {
    let status = if item.is_resolved { "✅" } else { "💬" };
    format!(
        "{status} {}:{} {} {} |{}|{}|{}|{}:{}|{}|{}",
        item.path,
        item.line_start,
        item.author,
        item.summary,
        item.id,
        item.url,
        item.path,
        item.line_start,
        item.line_end,
        item.revision,
        if item.is_resolved { "1" } else { "0" },
    )
}

fn parse_thread_item(item: &str) -> Result<ReviewThreadItem> {
    let mut parts = item.rsplitn(7, '|');
    let resolved = parts
        .next()
        .ok_or_else(|| anyhow!("missing resolved"))?
        .parse::<u8>()?;
    let revision = parts
        .next()
        .ok_or_else(|| anyhow!("missing revision"))?
        .to_string();
    let line_range = parts.next().ok_or_else(|| anyhow!("missing line range"))?;
    let (line_start, line_end) = line_range
        .split_once(':')
        .ok_or_else(|| anyhow!("invalid line range"))?;
    let line_start = line_start.parse::<usize>()?;
    let line_end = line_end.parse::<usize>()?;
    let path = parts
        .next()
        .ok_or_else(|| anyhow!("missing path"))?
        .to_string();
    let url = parts
        .next()
        .ok_or_else(|| anyhow!("missing url"))?
        .to_string();
    let id = parts
        .next()
        .ok_or_else(|| anyhow!("missing id"))?
        .to_string();
    let display = parts
        .next()
        .ok_or_else(|| anyhow!("missing display"))?
        .to_string();

    let (_, rest) = display
        .split_once(' ')
        .ok_or_else(|| anyhow!("invalid display"))?;
    let mut rest_parts = rest.splitn(3, ' ');
    let path_line = rest_parts
        .next()
        .ok_or_else(|| anyhow!("invalid display path"))?;
    let author = rest_parts
        .next()
        .ok_or_else(|| anyhow!("invalid display author"))?
        .to_string();
    let summary = rest_parts
        .next()
        .ok_or_else(|| anyhow!("invalid display summary"))?
        .trim()
        .to_string();
    let _ = path_line;

    Ok(ReviewThreadItem {
        id,
        url,
        path,
        line_start,
        line_end,
        revision,
        is_resolved: resolved == 1,
        author,
        summary,
        body: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_name_is_git_review() {
        assert_eq!(GitReview::new().name(), "pr-threads");
    }

    #[test]
    fn thread_item_roundtrip() {
        let item = ReviewThreadItem {
            id: "THREAD_1".to_string(),
            url: "https://example.com/thread/1".to_string(),
            path: "src/main.rs".to_string(),
            line_start: 40,
            line_end: 42,
            revision: "abc1234".to_string(),
            is_resolved: false,
            author: "alice".to_string(),
            summary: "fix typo".to_string(),
            body: "alice:\nfix typo".to_string(),
        };
        let rendered = render_thread_item(&item);
        let parsed = parse_thread_item(&rendered).unwrap();
        assert_eq!(parsed.id, item.id);
        assert_eq!(parsed.url, item.url);
        assert_eq!(parsed.path, item.path);
        assert_eq!(parsed.line_start, item.line_start);
        assert_eq!(parsed.line_end, item.line_end);
        assert_eq!(parsed.author, item.author);
        assert_eq!(parsed.summary, item.summary);
        assert_eq!(parsed.is_resolved, item.is_resolved);
    }

    #[test]
    fn summarize_comment_uses_first_non_empty_line() {
        let s = summarize_comment("\n\n first line \nsecond line");
        assert_eq!(s, "first line");
    }

    #[tokio::test]
    async fn preview_message_includes_code_block_and_body() {
        let thread = ReviewThreadItem {
            id: "THREAD_2".to_string(),
            url: "https://example.com/thread/2".to_string(),
            path: "src/lib.rs".to_string(),
            line_start: 9,
            line_end: 10,
            revision: "def5678".to_string(),
            is_resolved: true,
            author: "bob".to_string(),
            summary: "nit".to_string(),
            body: "bob:\nplease rename".to_string(),
        };
        let preview = compose_preview_text(&thread, "fn main() {}".to_string(), 80).await;
        assert!(preview.contains("fn main() {}"));
        assert!(preview.contains("bob"));
        assert!(preview.contains("✅ Resolved"));
        assert!(preview.contains("src/lib.rs:9-10"));
    }
}
