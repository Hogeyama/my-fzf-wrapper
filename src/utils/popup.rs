//! 独立した PTY を持つポップアップ端末でコマンドを実行する。
//!
//! サーバープロセスから直接 fzf や nvim を起動すると、親 fzf と /dev/tty を共有して
//! エスケープシーケンスが壊れる。ポップアップは独立した PTY を持つため、
//! この問題を根本的に回避できる。
//!
//! herdr 内 (`HERDR_ENV=1`) では herdr のポップアップを、それ以外では tmux popup を使う。
//! herdr では事前に `herdr plugin link <repo>/herdr-plugin` でプラグイン登録が必要。

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use anyhow::bail;
use anyhow::Result;
use tokio::process::Command;

const HERDR_PLUGIN_ID: &str = "fzfw.popup";
const HERDR_ENTRYPOINT: &str = "run";

/// シェルスクリプトをポップアップで実行し、ポップアップが閉じるまで待つ。
pub async fn run(script: &str, width: &str, height: &str) -> Result<()> {
    if in_herdr() {
        run_in_herdr(script, width, height).await
    } else {
        run_in_tmux(script, width, height).await
    }
}

/// ファイルをポップアップ内の nvim で編集し、閉じるまで待つ。
pub async fn edit_file(path: &Path) -> Result<()> {
    if in_herdr() {
        let script = format!("nvim {}", shell_quote(&path.to_string_lossy()));
        run_in_herdr(&script, "80%", "80%").await
    } else {
        // TODO make configurable?
        Command::new("nvimw")
            .arg("--tmux-popup")
            .arg(path)
            .spawn()?
            .wait()
            .await?;
        Ok(())
    }
}

/// シングルクォートでシェル用にエスケープする。
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn in_herdr() -> bool {
    std::env::var("HERDR_ENV").as_deref() == Ok("1")
}

async fn run_in_tmux(script: &str, width: &str, height: &str) -> Result<()> {
    Command::new("tmux")
        .args([
            "popup", "-E", "-w", width, "-h", height, "--", "sh", "-c", script,
        ])
        .spawn()?
        .wait()
        .await?;
    Ok(())
}

// herdr の plugin pane open はポップアップの起動直後に返り、終了を待つ手段も
// 提供されていない。そこでスクリプト終了時 (popup.close による HUP を含む) に
// マーカーファイルを作らせ、それを待つ。
async fn run_in_herdr(script: &str, width: &str, height: &str) -> Result<()> {
    let dir = tempfile::Builder::new().prefix("fzfw-popup-").tempdir()?;
    let done = dir.path().join("done");
    let wrapped = format!(
        "trap 'touch {done}' EXIT; trap 'exit 129' HUP INT TERM; {script}",
        done = shell_quote(&done.to_string_lossy()),
    );

    let output = Command::new("herdr")
        .args([
            "plugin",
            "pane",
            "open",
            "--plugin",
            HERDR_PLUGIN_ID,
            "--entrypoint",
            HERDR_ENTRYPOINT,
            "--placement",
            "popup",
            "--width",
            width,
            "--height",
            height,
        ])
        .arg("--env")
        .arg(format!("FZFW_POPUP_SCRIPT={wrapped}"))
        .stdin(Stdio::null())
        .output()
        .await?;
    // エラー内容は stdout に JSON で出る ({"error":{"code":...,"message":...}})
    if !output.status.success() {
        bail!(
            "herdr plugin pane open failed: {} {}",
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    while !done.exists() {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(())
}
