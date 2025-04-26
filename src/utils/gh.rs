use std::process::ExitStatus;

use anyhow::Result;
use tokio::process::Command;

pub async fn browse_github(file: impl AsRef<str>) -> Result<()> {
    let _: ExitStatus = Command::new("gh")
        .arg("browse")
        .arg(file.as_ref())
        .spawn()?
        .wait()
        .await?;
    Ok(())
}

pub async fn browse_github_line(
    file: impl AsRef<str>,
    revision: impl AsRef<str>,
    line: usize,
) -> Result<()> {
    let _: ExitStatus = Command::new("gh")
        .arg("browse")
        .arg(format!("{}:{}", file.as_ref(), line))
        .arg(format!("--commit={}", revision.as_ref()))
        .spawn()?
        .wait()
        .await?;
    Ok(())
}
