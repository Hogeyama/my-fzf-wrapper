use anyhow::Result;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

pub async fn render_file(file: impl AsRef<str>) -> Result<String> {
    let output = Command::new("bat")
        .args(vec!["--color", "always"])
        .arg(file.as_ref())
        .output()
        .await?
        .stdout;
    Ok(String::from_utf8_lossy(output.as_slice()).into_owned())
}

pub async fn render_file_with_highlight(file: impl AsRef<str>, line: isize) -> Result<String> {
    let start_line = std::cmp::max(0, line - 15);
    let output = Command::new("bat")
        .args(vec!["--color", "always"])
        .args(vec!["--line-range", &format!("{start_line}:")])
        .args(vec!["--highlight-line", &line.to_string()])
        .arg(file.as_ref())
        .output()
        .await?
        .stdout;
    Ok(String::from_utf8_lossy(output.as_slice()).into_owned())
}

pub async fn render_stdin_with_highlight_range(
    content: &[u8],
    file_name: &str,
    highlight_start: isize,
    highlight_end: isize,
    max_lines: usize,
    max_columns: usize,
) -> Result<String> {
    let columns = sanitize_columns(max_columns);
    let lines = sanitize_lines(max_lines);

    let center = std::cmp::max(1, (highlight_start + highlight_end) / 2);
    let radius = std::cmp::max(1, (lines as isize - 1) / 2);
    let start_line = std::cmp::max(1, center - radius);
    let end_line = std::cmp::max(start_line, center + radius);

    let mut child = Command::new("bat")
        .args(["--color", "always"])
        .args(["--wrap", "never"])
        .args(["--terminal-width", &columns.to_string()])
        .args(["--line-range", &format!("{start_line}:{end_line}")])
        .args([
            "--highlight-line",
            &format!("{highlight_start}:{highlight_end}"),
        ])
        .args(["--file-name", file_name])
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(content).await?;
    }

    let output = child.wait_with_output().await?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn sanitize_lines(lines: usize) -> usize {
    std::cmp::max(8, lines)
}

fn sanitize_columns(columns: usize) -> usize {
    std::cmp::max(40, columns)
}
