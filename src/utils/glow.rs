use tokio::{io::AsyncWriteExt, process::Command};

pub async fn render_markdown(md: String) -> Result<String, String> {
    let mut glow = Command::new("glow")
        .args(vec!["-s", "dark"])
        .args(vec!["-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut stdin = glow.stdin.take().unwrap();
    stdin.write_all(md.as_bytes()).await.unwrap();
    drop(stdin);
    let glow_output = glow.wait_with_output().await.map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(glow_output.stdout.as_slice()).to_string())
}
