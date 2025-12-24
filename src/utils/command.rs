use std::process::Output;
use std::process::Stdio;

use anyhow::Result;
use encoding_rs::Encoding;
use encoding_rs::EUC_JP;
use encoding_rs::SHIFT_JIS;
use encoding_rs::UTF_8;
use futures::Stream;
use futures::StreamExt;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
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
pub fn command_output_stream(command: Command) -> impl Stream<Item = Result<String>> {
    command_output_stream_with_encodings(command, vec![UTF_8, EUC_JP, SHIFT_JIS])
}

pub fn command_output_stream_with_encodings(
    mut command: Command,
    encodings: Vec<&'static Encoding>,
) -> impl Stream<Item = Result<String>> {
    async_stream::stream! {
        let mut child = command
            .stdout(Stdio::piped())
            .spawn()?;

        let stdout = child.stdout.take()
            .ok_or_else(|| anyhow::anyhow!("Failed to capture stdout"))?;

        let read_stream = async_stream::stream! {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut bytes = Vec::new();
                match reader.read_until(b'\n', &mut bytes).await {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        match decode(&bytes, encodings.clone()) {
                            Some(result) => yield Ok(result),
                            None => {
                                // ad-hoc fallback
                                yield Ok(UTF_8.decode(&bytes).0.trim_end().to_string())
                            }
                        }
                    },
                    Err(e) => yield Err(anyhow::anyhow!("Failed to read line: {}", e)),
                }
            }
        };
        tokio::pin!(read_stream);

        loop {
            tokio::select! {
                maybe_line = read_stream.next() => {
                    match maybe_line {
                        Some(line) => yield line,
                        None => break,
                    }
                }
                _ = signal::ctrl_c() => {
                    info!("Received SIGINT, terminating child process...");
                    if let Err(e) = child.kill().await {
                        eprintln!("Failed to kill child process: {}", e);
                    }
                    break;
                }
            }
        }
        match child.wait().await {
            Ok(status) if status.success() => {
                // nop
            }
            result => {
                info!("Child process exited with status: {:?}", result);
            }
        }
    }
}

fn decode(bytes: &[u8], encodings: Vec<&'static Encoding>) -> Option<String> {
    for &encoding in &encodings {
        let (cow, _, had_errors) = encoding.decode(bytes);
        if !had_errors {
            return Some(cow.trim_end().to_string());
        }
    }
    None
}
