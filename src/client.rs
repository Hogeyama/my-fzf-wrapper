// std
use std::error::Error;

// clap command line parser
use clap::Subcommand;

// Tokio
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::net::UnixStream;

use crate::method;
use crate::method::LoadResp;
use crate::method::Method;
use crate::method::PreviewResp;

/// internal
/// Subcommand called by fzf
#[derive(Subcommand)]
pub enum Command {
    /// internal
    Load {
        #[clap(long, env)]
        fzfw_socket: String,
        #[clap(flatten)]
        params: method::LoadParam,
    },
    /// internal
    Execute {
        #[clap(long, env)]
        fzfw_socket: String,
        #[clap(flatten)]
        params: method::ExecuteParam,
    },
    /// internal
    Preview {
        #[clap(long, env)]
        fzfw_socket: String,
        #[clap(flatten)]
        params: method::PreviewParam,
    },
    /// internal
    ChangeMode {
        #[clap(long, env)]
        fzfw_socket: String,
        #[clap(flatten)]
        params: method::ChangeModeParam,
    },
    /// internal
    ChangeDirectory {
        #[clap(long, env)]
        fzfw_socket: String,
        #[clap(flatten)]
        params: method::ChangeDirectoryParam,
    },
}

pub async fn run_command(command: Command) -> Result<(), Box<dyn Error>> {
    match command {
        Command::Load {
            fzfw_socket,
            params,
        } => {
            match send_request(fzfw_socket, method::Load, params).await? {
                Ok(LoadResp { header, items }) => {
                    println!("{}", header);
                    for line in items {
                        println!("{}", line);
                    }
                }
                Err(e) => println!("Error: {}", e),
            }
            Ok(())
        }
        Command::Execute {
            fzfw_socket,
            params,
        } => {
            match send_request(fzfw_socket, method::Execute, params).await? {
                Ok(_) => {}
                Err(e) => println!("Error: {}", e),
            }
            Ok(())
        }
        Command::Preview {
            fzfw_socket,
            params,
        } => {
            match send_request(fzfw_socket, method::Preview, params).await? {
                Ok(PreviewResp { message }) => println!("{}", message),
                Err(e) => println!("Error: {}", e),
            }
            Ok(())
        }
        Command::ChangeMode {
            fzfw_socket,
            params,
        } => {
            match send_request(fzfw_socket, method::ChangeMode, params).await? {
                Ok(_) => {}
                Err(e) => println!("Error: {}", e),
            }
            Ok(())
        }
        Command::ChangeDirectory {
            fzfw_socket,
            params,
        } => {
            match send_request(fzfw_socket, method::ChangeDirectory, params).await? {
                Ok(_) => {}
                Err(e) => println!("Error: {}", e),
            }
            Ok(())
        }
    }
}

pub async fn send_request<M: Method>(
    fzfw_socket: String,
    method: M,
    param: <M as method::Method>::Param,
) -> Result<Result<<M as method::Method>::Response, String>, Box<dyn Error>> {
    let (rx, mut tx) = tokio::io::split(UnixStream::connect(&fzfw_socket).await?);
    let mut rx = BufReader::new(rx).lines();

    let req = serde_json::to_string(&<M as Method>::request(method, param))?;
    tx.write_all(format!("{req}\n").as_bytes()).await?;

    let resp = rx.next_line().await?.unwrap();
    match serde_json::from_str(&resp) {
        Ok(resp) => Ok(Ok(resp)),
        Err(e) => Ok(Err(e.to_string())),
    }
}
