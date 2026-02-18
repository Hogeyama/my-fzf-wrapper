use anyhow::Result;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

pub async fn yank(str: impl AsRef<str>) -> Result<()> {
    let mut glow = Command::new("myclip")
        .stdin(std::process::Stdio::piped())
        .spawn()?;
    let mut stdin = glow.stdin.take().unwrap();
    stdin.write_all(str.as_ref().as_bytes()).await.unwrap();
    drop(stdin);
    Ok(())
}
