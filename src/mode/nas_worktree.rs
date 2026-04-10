use anyhow::anyhow;
use anyhow::Result;
use futures::future::BoxFuture;
use futures::FutureExt;
use tokio::process::Command;

use crate::env::Env;
use crate::method::LoadResp;
use crate::method::PreviewResp;
use crate::mode::config_builder;
use crate::mode::git_log;
use crate::mode::CallbackMap;
use crate::mode::ModeDef;
use crate::utils::fzf::PreviewWindow;
use crate::utils::xsel;

#[derive(Clone)]
pub struct NasWorktree;

#[derive(serde::Deserialize, Debug, Clone)]
struct WorktreeEntry {
    path: String,
    #[allow(dead_code)]
    branch: Option<String>,
    #[allow(dead_code)]
    head: String,
}

impl ModeDef for NasWorktree {
    fn name(&self) -> &'static str {
        "nas-worktree"
    }
    fn load(&self, _env: &Env, _query: String, _item: String) -> super::LoadStream {
        Box::pin(async_stream::stream! {
            let output = Command::new("nas")
                .args(["worktree", "list", "--format", "json"])
                .output()
                .await?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
                yield Err(anyhow!("nas worktree list failed: {}", stderr));
                return;
            }
            let entries: Vec<WorktreeEntry> = serde_json::from_slice(&output.stdout)?;
            let cwd = std::env::current_dir().ok();
            let cwd_ref = cwd.as_deref();
            let mut entries = entries;
            entries.sort_by_key(|e| {
                // 現在の cwd と一致する worktree を先頭に (false < true)
                cwd_ref.map(|c| std::path::Path::new(&e.path) != c).unwrap_or(true)
            });
            let items: Vec<String> = entries.into_iter().map(|e| e.path).collect();
            yield Ok(LoadResp::new_with_default_header(items))
        })
    }
    fn preview(
        &self,
        _env: &Env,
        _win: &PreviewWindow,
        path: String,
    ) -> BoxFuture<'static, Result<PreviewResp>> {
        async move {
            let branch_out = Command::new("git")
                .args(["-C", &path, "rev-parse", "--abbrev-ref", "HEAD"])
                .output()
                .await?;
            if !branch_out.status.success() {
                let stderr = String::from_utf8_lossy(&branch_out.stderr).into_owned();
                return Ok(PreviewResp {
                    message: format!("git rev-parse failed: {stderr}"),
                });
            }
            let branch_raw = String::from_utf8_lossy(&branch_out.stdout)
                .trim()
                .to_string();
            let branch = if branch_raw == "HEAD" {
                "(detached)".to_string()
            } else {
                branch_raw
            };

            let head_out = Command::new("git")
                .args(["-C", &path, "rev-parse", "HEAD"])
                .output()
                .await?;
            let head = String::from_utf8_lossy(&head_out.stdout).trim().to_string();

            let log_out = Command::new("git")
                .args([
                    "-C",
                    &path,
                    "log",
                    "--graph",
                    "--oneline",
                    "--decorate",
                    "-20",
                ])
                .output()
                .await?;
            let log = String::from_utf8_lossy(&log_out.stdout).into_owned();

            let message = format!("Path:   {path}\nBranch: {branch}\nHEAD:   {head}\n\n{log}");
            Ok(PreviewResp { message })
        }
        .boxed()
    }
    fn fzf_bindings(&self) -> (super::ModeBindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [
                execute_silent!(b, |_mode, _env, _query, path| {
                    std::env::set_current_dir(&path)?;
                    Ok(())
                }),
                b.change_mode(git_log::GitLog::Head.name(), false),
            ],
            "ctrl-y" => [
                execute!(b, |_mode, _env, _query, path| {
                    xsel::yank(path).await?;
                    Ok(())
                }),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_worktree_entry_with_branch() {
        let json = r#"[{"path":"/tmp/a","branch":"foo","head":"abc123"}]"#;
        let entries: Vec<WorktreeEntry> = serde_json::from_str(json).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "/tmp/a");
        assert_eq!(entries[0].branch.as_deref(), Some("foo"));
        assert_eq!(entries[0].head, "abc123");
    }

    #[test]
    fn parse_worktree_entry_detached() {
        let json = r#"[{"path":"/tmp/b","branch":null,"head":"def456"}]"#;
        let entries: Vec<WorktreeEntry> = serde_json::from_str(json).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].branch.is_none());
    }

    #[test]
    fn sort_puts_cwd_first() {
        let mut entries = vec![
            WorktreeEntry {
                path: "/tmp/a".to_string(),
                branch: None,
                head: "h1".to_string(),
            },
            WorktreeEntry {
                path: "/tmp/b".to_string(),
                branch: None,
                head: "h2".to_string(),
            },
            WorktreeEntry {
                path: "/tmp/c".to_string(),
                branch: None,
                head: "h3".to_string(),
            },
        ];
        let cwd = std::path::Path::new("/tmp/b");
        entries.sort_by_key(|e| std::path::Path::new(&e.path) != cwd);
        assert_eq!(entries[0].path, "/tmp/b");
    }
}
