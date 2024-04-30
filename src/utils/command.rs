use std::process::Output;

use tokio::process::Command;

pub async fn edit_and_run(
    placeholder: impl AsRef<[u8]>,
) -> Result<(String, Output), std::io::Error> {
    let tmp_file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp_file.path(), placeholder).unwrap();
    // TODO make configurable?
    Command::new("nvimw")
        .arg("--tmux-popup")
        .arg(tmp_file.path())
        .spawn()?
        .wait()
        .await?;
    let cmd = std::fs::read_to_string(tmp_file.path()).unwrap();
    let output = Command::new("sh").arg("-c").arg(&cmd).output().await?;
    Ok((cmd, output))
}
