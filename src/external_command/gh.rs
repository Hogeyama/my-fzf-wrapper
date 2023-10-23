use std::process::ExitStatus;

use tokio::process::Command;

pub async fn browse_github(file: impl AsRef<str>) -> Result<(), String> {
    let _: ExitStatus = Command::new("gh")
        .arg("browse")
        .arg(&format!("{}", file.as_ref()))
        .spawn()
        .map_err(|e| e.to_string())?
        .wait()
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn browse_github_line(
    file: impl AsRef<str>,
    revision: impl AsRef<str>,
    line: usize,
) -> Result<(), String> {
    let _: ExitStatus = Command::new("gh")
        .arg("browse")
        .arg(&format!("{}:{}", file.as_ref(), line))
        .arg(&format!("--commit={}", revision.as_ref()))
        .spawn()
        .map_err(|e| e.to_string())?
        .wait()
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
