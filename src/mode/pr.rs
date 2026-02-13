use anyhow::anyhow;
use anyhow::Result;
use futures::future::BoxFuture;
use futures::FutureExt;
use serde::Deserialize;
use tokio::process::Command;

use crate::config::Config;
use crate::method::LoadResp;
use crate::method::PreviewResp;
use crate::mode::config_builder;
use crate::mode::CallbackMap;
use crate::mode::ModeDef;
use crate::nvim::NeovimExt;
use crate::state::State;
use crate::utils::fzf;
use crate::utils::fzf::PreviewWindow;

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

impl ModeDef for GhPr {
    fn name(&self) -> &'static str {
        match self {
            GhPr::Open => "gh-pr",
            GhPr::All => "gh-pr(all)",
        }
    }

    fn load<'a>(
        &'a self,
        _config: &'a Config,
        _state: &'a mut State,
        _query: String,
        _item: String,
    ) -> super::LoadStream<'a> {
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
            let items: Vec<String> = prs
                .iter()
                .map(|pr| {
                    format!(
                        "#{}\t{}\t{}\t{}\t[{}]",
                        pr.number, pr.state, pr.head_ref_name, pr.title, pr.author.login
                    )
                })
                .collect();
            yield Ok(LoadResp::new_with_default_header(items))
        })
    }

    fn preview(
        &self,
        _config: &Config,
        _win: &PreviewWindow,
        item: String,
    ) -> BoxFuture<'static, Result<PreviewResp>> {
        async move {
            let number = parse_pr_number(&item)?;
            let output = Command::new("gh")
                .args(["pr", "view", &number])
                .output()
                .await?;
            let message = String::from_utf8_lossy(&output.stdout).to_string();
            Ok(PreviewResp { message })
        }
        .boxed()
    }

    fn fzf_bindings(&self) -> (fzf::Bindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [
                execute_silent!{b, |_mode,_config,_state,_query,item| {
                    let number = parse_pr_number(&item)?;
                    Command::new("gh")
                        .args(["pr", "view", "--web", &number])
                        .spawn()?
                        .wait()
                        .await?;
                    Ok(())
                }}
            ],
            "pgup" => [
                select_and_execute!{b, |_mode,config,_state,_query,item|
                    "browse" => {
                        let number = parse_pr_number(&item)?;
                        Command::new("gh")
                            .args(["pr", "view", "--web", &number])
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
                        config.nvim.notify_command_result("gh pr checkout", output)
                            .await
                    },
                },
                b.reload(),
            ],
        }
    }

    fn fzf_extra_opts(&self) -> Vec<&str> {
        vec!["--no-sort"]
    }
}

fn parse_pr_number(item: &str) -> Result<String> {
    item.split('\t')
        .next()
        .and_then(|s| s.strip_prefix('#'))
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("Failed to parse PR number from: {}", item))
}
