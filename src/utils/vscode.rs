use anyhow::Result;
use std::process::Output;
use tokio::process::Command;

pub fn in_vscode() -> bool {
    std::env::var("VSCODE_INJECTION").is_ok()
}

pub async fn open(path: String, line: Option<usize>) -> Result<Output> {
    let pathline = match line {
        Some(line) => format!("{}:{}", path, line),
        None => path,
    };
    let output = Command::new("code")
        .arg("-g")
        .arg(pathline)
        .output()
        .await?;
    Ok(output)
}
