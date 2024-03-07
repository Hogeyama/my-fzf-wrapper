use tokio::process::Command;

pub async fn render_file(file: impl AsRef<str>) -> Result<String, String> {
    let output = Command::new("bat")
        .args(vec!["--color", "always"])
        .arg(file.as_ref())
        .output()
        .await
        .map_err(|e| e.to_string())?
        .stdout;
    Ok(String::from_utf8_lossy(output.as_slice()).into_owned())
}

pub async fn render_file_with_highlight(
    file: impl AsRef<str>,
    line: isize,
) -> Result<String, String> {
    let start_line = std::cmp::max(0, line - 15);
    let output = Command::new("bat")
        .args(vec!["--color", "always"])
        .args(vec!["--line-range", &format!("{start_line}:")])
        .args(vec!["--highlight-line", &line.to_string()])
        .arg(file.as_ref())
        .output()
        .await
        .map_err(|e| e.to_string())?
        .stdout;
    Ok(String::from_utf8_lossy(output.as_slice()).into_owned())
}
