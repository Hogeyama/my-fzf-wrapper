use tokio::{io::AsyncWriteExt, process::Command};

pub async fn yank(str: impl AsRef<str>) -> Result<(), String> {
    let mut glow = Command::new("xsel")
        .args(vec!["-b"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut stdin = glow.stdin.take().unwrap();
    stdin.write_all(str.as_ref().as_bytes()).await.unwrap();
    drop(stdin);
    Ok(())
}
