use std::process::Output;
use std::process::Stdio;

use anyhow::Result;
use futures::Stream;
use futures::StreamExt;
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;
use tokio::signal;

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
    let cmd = std::fs::read_to_string(tmp_file.path())
        .unwrap()
        .trim()
        .to_string();
    let output = Command::new("sh").arg("-c").arg(&cmd).output().await?;
    Ok((cmd, output))
}

pub fn command_output_stream(mut command: Command) -> impl Stream<Item = Result<String>> {
    async_stream::stream! {
        let mut child = command
            .stdout(Stdio::piped())
            .spawn()?;
        let stdout = child.stdout.take()
            .ok_or_else(|| anyhow::anyhow!("Failed to capture stdout"))?;

        let mut reader = tokio::io::BufReader::new(stdout).lines();

        let read_stream = async_stream::stream! {
            while let Some(line) = reader.next_line().await.transpose() {
                yield line.map_err(|e| anyhow::anyhow!("Failed to read line: {}", e))
            }
        };
        tokio::pin!(read_stream);

        loop {
            tokio::select! {
                Some(line) = read_stream.next() => {
                    yield line;
                }
                result = child.wait() => {
                    info!("Child process exited with status: {:?}", result);
                    break;
                }
                _ = signal::ctrl_c() => {
                    println!("Received SIGINT, terminating child process...");
                    if let Err(e) = child.kill().await {
                        eprintln!("Failed to kill child process: {}", e);
                    }
                    break;
                }
            }
        }
    }
}
