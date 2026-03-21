#![allow(dead_code)]
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::process::Command;

pub struct Config {
    pub default_command: String,
    pub preview_command: String,
    pub socket: String,
    pub log_file: String,
    pub initial_prompt: String,
    pub initial_query: String,
    pub rendered_bindings: HashMap<String, String>,
    pub extra_opts: Vec<String>,
    pub listen_socket: Option<String>,
}

#[derive(Clone)]
pub enum Action {
    Reload(String),
    Execute(String),
    ExecuteSilent(String),
    ChangePrompt(String),
    ToggleSort,
    EnableSearch,
    DisableSearch,
    ChangePreviewWindow(String),
    DeselectAll,
    ClearQuery,
    ClearScreen,
    First,
    Toggle,
    Raw(String),
}

impl Action {
    pub fn render(&self) -> String {
        match self {
            Action::Reload(cmd) => format!("reload[{cmd}]"),
            Action::Execute(cmd) => format!("execute[{cmd}]"),
            Action::ExecuteSilent(cmd) => format!("execute-silent[{cmd}]"),
            Action::ChangePrompt(prompt) => format!("change-prompt[{prompt}]"),
            Action::ToggleSort => "toggle-sort".to_string(),
            Action::EnableSearch => "enable-search".to_string(),
            Action::DisableSearch => "disable-search".to_string(),
            Action::ChangePreviewWindow(spec) => {
                format!("change-preview-window[{spec}]")
            }
            Action::DeselectAll => "deselect-all".to_string(),
            Action::ClearQuery => "clear-query".to_string(),
            Action::ClearScreen => "clear-screen".to_string(),
            Action::First => "first".to_string(),
            Action::Toggle => "toggle".to_string(),
            Action::Raw(s) => s.to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct PreviewWindow {
    pub lines: usize,
    pub columns: usize,
}

impl PreviewWindow {
    pub fn from_env() -> Option<Self> {
        let lines = std::env::var("FZF_PREVIEW_LINES").ok()?.parse().ok()?;
        let columns = std::env::var("FZF_PREVIEW_COLUMNS").ok()?.parse().ok()?;
        Some(Self { lines, columns })
    }
}

pub fn new(config: Config) -> Command {
    let Config {
        default_command,
        preview_command,
        socket,
        log_file,
        initial_prompt,
        initial_query,
        rendered_bindings,
        extra_opts,
        listen_socket,
    } = config;
    let mut fzf = Command::new("fzf");
    fzf.kill_on_drop(true);

    fzf.env("FZF_DEFAULT_COMMAND", default_command);
    fzf.env("FZFW_LOG_FILE", log_file);
    fzf.env("FZFW_SOCKET", socket);

    let c = |s: &str| s.to_string();

    #[rustfmt::skip]
    let mut args = vec![
        c("--ansi"),
        c("--header-lines"), c("1"),
        c("--layout"), c("reverse"),
        c("--query"), initial_query,
        c("--preview"), preview_command,
        c("--preview-window"), c("right:50%:noborder"),
        c("--prompt"), initial_prompt
    ];

    for (key, actions) in &rendered_bindings {
        args.push("--bind".to_string());
        args.push(format!("{key}:{actions}"));
    }

    if let Some(ref listen_socket) = listen_socket {
        args.push(format!("--listen={}", listen_socket));
    }

    extra_opts.iter().for_each(|opt| {
        args.push(opt.to_string());
    });

    fzf.args(args);

    fzf
}

// ---------------------------------------------------------------------------
// tmux popup 経由の fzf select
// ---------------------------------------------------------------------------
//
// サーバープロセスから直接 fzf を起動すると、親 fzf と /dev/tty を共有して
// エスケープシーケンスが壊れる。tmux popup は独立した PTY を持つため、
// この問題を根本的に回避できる。

/// tmux popup 内で fzf を実行する共通実装。
/// 入出力はテンポラリファイルで受け渡す。
async fn run_fzf_in_popup(items: &str, extra_args: &[&str]) -> Result<String> {
    let input_file = tempfile::Builder::new().prefix("fzf-in-").tempfile()?;
    let output_file = tempfile::Builder::new().prefix("fzf-out-").tempfile()?;
    std::fs::write(input_file.path(), items)?;

    let input_path = input_file.path().to_string_lossy();
    let output_path = output_file.path().to_string_lossy();

    // 各引数をシングルクォートでエスケープしてシェルに渡す
    let extra = extra_args
        .iter()
        .map(|a| format!("'{}'", a.replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join(" ");

    let script =
        format!("fzf --ansi --no-sort --layout=reverse {extra} < '{input_path}' > '{output_path}'");

    Command::new("tmux")
        .args([
            "popup", "-E", "-w", "80%", "-h", "50%", "--", "sh", "-c", &script,
        ])
        .spawn()?
        .wait()
        .await?;

    Ok(std::fs::read_to_string(output_file.path())?
        .trim()
        .to_string())
}

pub async fn select_multi(items: Vec<&str>) -> Result<Vec<String>> {
    select_multi_with_args(items, &[]).await
}

pub async fn select_multi_with_args(items: Vec<&str>, extra_args: &[&str]) -> Result<Vec<String>> {
    let mut args = vec!["--multi"];
    args.extend_from_slice(extra_args);
    let output = run_fzf_in_popup(&items.join("\n"), &args).await?;
    if output.is_empty() {
        return Ok(vec![]);
    }
    Ok(output.lines().map(|s| s.to_string()).collect())
}

pub async fn select(items: Vec<&str>) -> Result<String> {
    run_fzf_in_popup(&items.join("\n"), &[]).await
}

pub async fn select_with_header(header: impl AsRef<str>, items: Vec<&str>) -> Result<String> {
    let input = format!("{}\n{}", header.as_ref(), items.join("\n"));
    run_fzf_in_popup(&input, &["--header-lines", "1"]).await
}

// ---------------------------------------------------------------------------
// シンプル入力
// ---------------------------------------------------------------------------

pub async fn input(header: impl AsRef<str>) -> Result<String> {
    input_with_placeholder(header, "").await
}

pub async fn input_with_placeholder(
    header: impl AsRef<str>,
    placeholder: impl AsRef<str>,
) -> Result<String> {
    let fzf = Command::new("fzf")
        .arg("--ansi")
        .args(vec!["--header", header.as_ref()])
        .args(vec!["--layout", "reverse"])
        .args(vec!["--bind", "enter:print-query"])
        .args(vec!["--query", placeholder.as_ref()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .spawn()?;

    Ok(
        String::from_utf8_lossy(&fzf.wait_with_output().await?.stdout)
            .trim()
            .to_string(),
    )
}

// ------------------------------------------------------------------------------
// FzfClient: fzf の --listen ソケットに HTTP リクエストを送るクライアント

pub struct FzfClient {
    socket_path: PathBuf,
}

impl FzfClient {
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// fzf にアクションを送信する (POST /)
    pub async fn post_action(&self, action: &str) -> Result<()> {
        let mut stream = UnixStream::connect(&self.socket_path).await?;

        let body = action.as_bytes();
        let request = format!(
            "POST / HTTP/1.0\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        stream.write_all(request.as_bytes()).await?;
        stream.write_all(body).await?;

        // レスポンスを読んでステータスコードを確認
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await?;
        let response_str = String::from_utf8_lossy(&response);
        if let Some(status_line) = response_str.lines().next() {
            if !status_line.contains("200") {
                return Err(anyhow::anyhow!("fzf --listen POST failed: {}", status_line));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_reload() {
        assert_eq!(Action::Reload("cmd".into()).render(), "reload[cmd]");
    }

    #[test]
    fn render_execute() {
        assert_eq!(Action::Execute("cmd".into()).render(), "execute[cmd]");
    }

    #[test]
    fn render_execute_silent() {
        assert_eq!(
            Action::ExecuteSilent("cmd".into()).render(),
            "execute-silent[cmd]"
        );
    }

    #[test]
    fn render_change_prompt() {
        assert_eq!(
            Action::ChangePrompt("foo>".into()).render(),
            "change-prompt[foo>]"
        );
    }

    #[test]
    fn render_simple_actions() {
        assert_eq!(Action::ToggleSort.render(), "toggle-sort");
        assert_eq!(Action::EnableSearch.render(), "enable-search");
        assert_eq!(Action::DisableSearch.render(), "disable-search");
        assert_eq!(Action::DeselectAll.render(), "deselect-all");
        assert_eq!(Action::ClearQuery.render(), "clear-query");
        assert_eq!(Action::ClearScreen.render(), "clear-screen");
        assert_eq!(Action::First.render(), "first");
        assert_eq!(Action::Toggle.render(), "toggle");
    }

    #[test]
    fn render_change_preview_window() {
        assert_eq!(
            Action::ChangePreviewWindow("right:50%".into()).render(),
            "change-preview-window[right:50%]"
        );
    }

    #[test]
    fn render_raw() {
        assert_eq!(
            Action::Raw("custom-action".into()).render(),
            "custom-action"
        );
    }
}
