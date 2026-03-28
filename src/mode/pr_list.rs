use anyhow::anyhow;
use anyhow::Result;
use futures::future::BoxFuture;
use futures::FutureExt;
use serde::Deserialize;
use tokio::process::Command;

use crate::env::Env;
use crate::method::LoadResp;
use crate::method::PreviewResp;
use crate::mode::config_builder;
use crate::mode::CallbackMap;
use crate::mode::ModeDef;
use crate::nvim::NeovimExt;
use crate::utils::fzf::PreviewWindow;
use crate::utils::glow;

#[derive(Clone)]
pub enum GhPr {
    Open,
    All,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhPrItem {
    number: u64,
    title: String,
    author: GhAuthor,
    head_ref_name: String,
    state: String,
}

#[derive(Deserialize)]
struct GhAuthor {
    login: String,
}

fn state_icon(state: &str) -> &'static str {
    match state {
        "OPEN" => "⬜",
        "MERGED" => "✅",
        "CLOSED" => "❌",
        _ => "❓",
    }
}

fn render_pr_item(pr: &GhPrItem) -> String {
    format!(
        "{}#{} {} by {} {}",
        state_icon(&pr.state),
        pr.number,
        pr.title,
        pr.author.login,
        pr.head_ref_name
    )
}

impl ModeDef for GhPr {
    fn name(&self) -> &'static str {
        match self {
            GhPr::Open => "pr-list(open)",
            GhPr::All => "pr-list(all)",
        }
    }

    fn load<'a>(&'a self, _env: &'a Env, _query: String, _item: String) -> super::LoadStream<'a> {
        Box::pin(async_stream::stream! {
            let mut cmd = Command::new("gh");
            cmd.args(["pr", "list", "--json", "number,title,author,headRefName,state", "--limit", "100"]);
            if matches!(self, GhPr::All) {
                cmd.args(["--state", "all"]);
            }
            let output = cmd.output().await?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                yield Err(anyhow!("gh pr list failed: {}", stderr));
                return;
            }
            let prs: Vec<GhPrItem> = serde_json::from_slice(&output.stdout)?;
            let items: Vec<String> = prs.iter().map(render_pr_item).collect();
            yield Ok(LoadResp::new_with_default_header(items))
        })
    }

    fn preview(
        &self,
        _env: &Env,
        _win: &PreviewWindow,
        item: String,
    ) -> BoxFuture<'static, Result<PreviewResp>> {
        async move {
            let number = parse_pr_number(&item)?;
            let output = Command::new("gh")
                .args(gh_pr_view_body_args(&number))
                .output()
                .await?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow!("gh pr view body failed: {}", stderr));
            }
            let body = String::from_utf8_lossy(&output.stdout).to_string();
            let message = glow::render_markdown(preview_body_text(&body)).await?;
            Ok(PreviewResp { message })
        }
        .boxed()
    }

    fn fzf_bindings(&self) -> (super::ModeBindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [
                execute_silent!{b, |_mode,_env,_query,item| {
                    let number = parse_pr_number(&item)?;
                    Command::new("gh")
                        .args(["pr", "view", "--web", &number])
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()?
                        .wait()
                        .await?;
                    Ok(())
                }}
            ],
            "pgup" => [
                select_and_execute!{b, |_mode,env,_query,item|
                    "browse" => {
                        let number = parse_pr_number(&item)?;
                        Command::new("gh")
                            .args(["pr", "view", "--web", &number])
                            .stdin(std::process::Stdio::null())
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn()?
                            .wait()
                            .await?;
                        Ok(())
                    },
                    "checkout" => {
                        let number = parse_pr_number(&item)?;
                        let output = Command::new("gh")
                            .args(["pr", "checkout", &number])
                            .output()
                            .await?;
                        env.nvim.notify_command_result("gh pr checkout", output)
                            .await
                    },
                },
                b.reload(),
            ],
        }
    }

    fn wants_sort(&self) -> bool {
        false
    }
}

fn parse_pr_number(item: &str) -> Result<String> {
    item.split_whitespace()
        .next()
        .and_then(|s| s.split('#').nth(1))
        .and_then(|s| s.parse::<u64>().ok())
        .map(|n| n.to_string())
        .or_else(|| {
            item.split('\t')
                .next()
                .and_then(|s| s.strip_prefix('#'))
                .and_then(|s| s.parse::<u64>().ok())
                .map(|n| n.to_string())
        })
        .or_else(|| {
            item.split_whitespace()
                .find_map(|tok| tok.strip_prefix('#'))
                .and_then(|s| s.parse::<u64>().ok())
                .map(|n| n.to_string())
        })
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("Failed to parse PR number from: {}", item))
}

fn gh_pr_view_body_args(number: &str) -> Vec<String> {
    vec![
        "pr".to_string(),
        "view".to_string(),
        number.to_string(),
        "--json".to_string(),
        "body".to_string(),
        "--jq".to_string(),
        ".body".to_string(),
    ]
}

fn preview_body_text(body: &str) -> String {
    let body = body.trim();
    if body.is_empty() {
        "(No description)".to_string()
    } else {
        body.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_pr_item_format() {
        let pr = GhPrItem {
            number: 42,
            title: "Fix login bug".into(),
            author: GhAuthor {
                login: "alice".into(),
            },
            head_ref_name: "fix/login".into(),
            state: "OPEN".into(),
        };
        assert_eq!(
            render_pr_item(&pr),
            "⬜#42 Fix login bug by alice fix/login"
        );
    }

    #[test]
    fn render_pr_item_merged() {
        let pr = GhPrItem {
            number: 100,
            title: "Add feature".into(),
            author: GhAuthor {
                login: "bob".into(),
            },
            head_ref_name: "feat/new".into(),
            state: "MERGED".into(),
        };
        assert_eq!(render_pr_item(&pr), "✅#100 Add feature by bob feat/new");
    }

    #[test]
    fn state_icon_unknown() {
        assert_eq!(state_icon("DRAFT"), "❓");
    }

    #[test]
    fn parse_pr_number_simple() {
        assert_eq!(
            parse_pr_number("⬜#42 title by author branch").unwrap(),
            "42"
        );
    }

    #[test]
    fn parse_pr_number_fails_on_garbage() {
        assert!(parse_pr_number("no number here").is_err());
    }

    #[test]
    fn preview_body_non_empty() {
        assert_eq!(preview_body_text("Hello world"), "Hello world");
    }

    #[test]
    fn preview_body_with_whitespace() {
        assert_eq!(preview_body_text("  Hello  "), "Hello");
    }

    #[test]
    fn wants_sort_is_false() {
        assert!(!GhPr::Open.wants_sort());
        assert!(!GhPr::All.wants_sort());
    }

    #[test]
    fn mode_name_is_git_pr_open_for_open_variant() {
        assert_eq!(GhPr::Open.name(), "pr-list(open)");
    }

    #[test]
    fn mode_name_is_git_pr_all_for_all_variant() {
        assert_eq!(GhPr::All.name(), "pr-list(all)");
    }

    #[test]
    fn preview_command_fetches_only_pr_body() {
        assert_eq!(
            gh_pr_view_body_args("123"),
            vec!["pr", "view", "123", "--json", "body", "--jq", ".body"]
                .into_iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn parse_pr_number_from_new_format() {
        assert_eq!(
            parse_pr_number("⬜#157 refs #502625 suppressMissing by Tom feat/502625").unwrap(),
            "157"
        );
    }

    #[test]
    fn parse_pr_number_ignores_by_inside_author_or_title() {
        assert_eq!(
            parse_pr_number("✅#88 Fix by keyword handling by John by smith feat/by-safe").unwrap(),
            "88"
        );
    }

    #[test]
    fn state_icon_mapping() {
        assert_eq!(state_icon("OPEN"), "⬜");
        assert_eq!(state_icon("MERGED"), "✅");
        assert_eq!(state_icon("CLOSED"), "❌");
    }

    #[test]
    fn preview_body_fallback_text_when_empty() {
        assert_eq!(preview_body_text(""), "(No description)");
        assert_eq!(preview_body_text("   "), "(No description)");
    }
}
