use anyhow::anyhow;
use anyhow::Result;
use futures::future::BoxFuture;
use futures::FutureExt;
use serde::Deserialize;
use tokio::process::Command;

use super::lib::actions;
use crate::config::Config;
use crate::method::LoadResp;
use crate::method::PreviewResp;
use crate::mode::config_builder;
use crate::mode::CallbackMap;
use crate::mode::ModeDef;
use crate::state::State;
use crate::utils::bat;
use crate::utils::browser;
use crate::utils::fzf;
use crate::utils::fzf::PreviewWindow;
use crate::utils::git;

#[derive(Clone)]
pub struct GitReview;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReviewThreadItem {
    id: String,
    url: String,
    path: String,
    line_start: usize,
    line_end: usize,
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
}

#[derive(Deserialize)]
struct ReviewCommentAuthor {
    login: String,
}

impl ModeDef for GitReview {
    fn name(&self) -> &'static str {
        "git-review"
    }

    fn load<'a>(
        &'a self,
        _config: &'a Config,
        _state: &'a mut State,
        _query: String,
        _item: String,
    ) -> super::LoadStream<'a> {
        Box::pin(async_stream::stream! {
            let threads = fetch_review_threads().await?;
            let items = threads.iter().map(render_thread_item).collect::<Vec<_>>();
            yield Ok(LoadResp::new_with_default_header(items));
        })
    }

    fn preview(
        &self,
        _config: &Config,
        win: &PreviewWindow,
        item: String,
    ) -> BoxFuture<'static, Result<PreviewResp>> {
        let win = *win;
        async move {
            let thread = parse_thread_item(&item)?;
            let workdir = git::workdir()?;
            let file = format!("{}{}", workdir, thread.path);
            let code = bat::render_file_with_highlight_range_and_window(
                &file,
                thread.line_start as isize,
                thread.line_end as isize,
                win.lines,
                win.columns,
            )
            .await
            .unwrap_or_else(|_| "(code preview unavailable)".to_string());
            let message = compose_preview_text(&thread, code);
            Ok(PreviewResp { message })
        }
        .boxed()
    }

    fn fzf_bindings(&self) -> (fzf::Bindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [
                execute!{b, |_mode,config,_state,_query,item| {
                    let thread = parse_thread_item(&item)?;
                    let workdir = git::workdir()?;
                    let file = format!("{}{}", workdir, thread.path);
                    actions::open_in_nvim(config, file, Some(thread.line_start), false).await
                }}
            ],
            "pgup" => [
                select_and_execute!{b, |_mode,_config,_state,_query,item|
                    "browse" => {
                        let thread = parse_thread_item(&item)?;
                        browser::open(&thread.url).await
                    },
                }
            ],
        }
    }

    fn fzf_extra_opts(&self) -> Vec<&str> {
        vec!["--no-sort"]
    }
}

fn compose_preview_text(thread: &ReviewThreadItem, code: String) -> String {
    format!(
        "[{}] {}:{}-{}\n{}\n\n{}\n\n---\n\n{}",
        if thread.is_resolved {
            "resolved"
        } else {
            "unresolved"
        },
        thread.path,
        thread.line_start,
        thread.line_end,
        thread.summary,
        code.trim_end_matches('\n'),
        thread.body
    )
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
            let line_start = t.start_line.or(t.original_start_line).unwrap_or(1);
            let line_end = t.line.or(t.original_line).unwrap_or(line_start);
            let line_start = usize::try_from(line_start).ok()?;
            let line_end = usize::try_from(line_end).ok()?;
            let first = t.comments.nodes.first()?;
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
    "query($owner:String!, $repo:String!, $pr:Int!) { repository(owner:$owner, name:$repo) { pullRequest(number:$pr) { reviewThreads(first:100) { nodes { id isResolved path line originalLine startLine originalStartLine comments(first:30) { nodes { url body author { login } } } } } } } }"
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
        "{status} {}:{} {} {} |{}|{}|{}|{}:{}|{}",
        item.path,
        item.line_start,
        item.author,
        item.summary,
        item.id,
        item.url,
        item.path,
        item.line_start,
        item.line_end,
        if item.is_resolved { "1" } else { "0" },
    )
}

fn parse_thread_item(item: &str) -> Result<ReviewThreadItem> {
    let mut parts = item.rsplitn(6, '|');
    let resolved = parts
        .next()
        .ok_or_else(|| anyhow!("missing resolved"))?
        .parse::<u8>()?;
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
        assert_eq!(GitReview.name(), "git-review");
    }

    #[test]
    fn thread_item_roundtrip() {
        let item = ReviewThreadItem {
            id: "THREAD_1".to_string(),
            url: "https://example.com/thread/1".to_string(),
            path: "src/main.rs".to_string(),
            line_start: 40,
            line_end: 42,
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

    #[test]
    fn preview_message_includes_code_block_and_body() {
        let thread = ReviewThreadItem {
            id: "THREAD_2".to_string(),
            url: "https://example.com/thread/2".to_string(),
            path: "src/lib.rs".to_string(),
            line_start: 9,
            line_end: 10,
            is_resolved: true,
            author: "bob".to_string(),
            summary: "nit".to_string(),
            body: "bob:\nplease rename".to_string(),
        };
        let preview = compose_preview_text(&thread, "fn main() {}".to_string());
        assert!(preview.contains("fn main() {}"));
        assert!(preview.contains("bob:"));
    }
}
