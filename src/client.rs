use std::env;
// std
use std::error::Error;

// clap command line parser
use clap::Subcommand;

// Tokio
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::net::UnixStream;

use crate::external_command;
use crate::logger::Serde;
use crate::method;
use crate::method::LoadResp;
use crate::method::Method;
use crate::method::PreviewParam;
use crate::method::PreviewResp;
use crate::method::RunParam;
use crate::method::RunResp;
use crate::mode;
use crate::types::Mode;

/// Subcommand called by the parent process (=fzf)
#[derive(Subcommand)]
pub enum Command {
    /// internal
    Load {
        #[clap(long, env)]
        fzfw_socket: String,
        mode: String,
        args: Vec<String>,
    },
    /// internal
    Reload {
        #[clap(long, env)]
        fzfw_socket: String,
    },
    /// internal
    Preview {
        #[clap(long, env)]
        fzfw_socket: String,
        item: String,
    },
    /// internal
    Run {
        #[clap(long, env)]
        fzfw_socket: String,
        item: String,
        args: Vec<String>,
    },
    /// internal
    LiveGrep {
        #[clap(long, env)]
        fzfw_socket: String,
        #[clap(subcommand)]
        subcommand: LiveGrepSubCommand,
    },
}

#[derive(Subcommand)]
pub enum LiveGrepSubCommand {
    /// internal
    Start,
    /// internal
    Update { query: String },
    /// internal
    GetResult,
}

pub async fn run_command(command: Command) -> Result<(), Box<dyn Error>> {
    match command {
        Command::Load {
            fzfw_socket,
            mode,
            args,
        } => {
            let (mut rx, mut tx) = tokio::io::split(UnixStream::connect(&fzfw_socket).await?);
            let args = method::LoadParam { mode, args };
            let resp = send_request(&mut tx, &mut rx, method::Load, args).await?;
            match resp {
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
        Command::Reload { fzfw_socket } => {
            let (mut rx, mut tx) = tokio::io::split(UnixStream::connect(&fzfw_socket).await?);
            let resp = send_request(&mut tx, &mut rx, method::Reload, ()).await?;
            match resp {
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
        Command::Preview { fzfw_socket, item } => {
            let param = PreviewParam { item };
            let (mut rx, mut tx) = tokio::io::split(UnixStream::connect(&fzfw_socket).await?);
            let resp = send_request(&mut tx, &mut rx, method::Preview, param).await?;
            match resp {
                Ok(PreviewResp { message }) => println!("{}", message),
                Err(e) => println!("Error: {}", e),
            }
            Ok(())
        }
        Command::Run {
            fzfw_socket,
            item,
            args,
        } => {
            let param = RunParam { item, args };
            let (mut rx, mut tx) = tokio::io::split(UnixStream::connect(&fzfw_socket).await?);
            let resp = send_request(&mut tx, &mut rx, method::Run, param).await?;
            match resp {
                Ok(RunResp) => {}
                Err(e) => println!("Error: {}", e),
            }
            Ok(())
        }
        Command::LiveGrep {
            fzfw_socket,
            subcommand,
        } => {
            match subcommand {
                LiveGrepSubCommand::Start => {
                    info!("Starting livegrep");
                    let myself = env::current_exe()
                        .expect("$0")
                        .to_string_lossy()
                        .into_owned();
                    external_command::fzf::new_livegrep(myself, &fzfw_socket)
                        .spawn()
                        .expect("Failed to spawn fzf")
                        .wait()
                        .await?;
                }
                LiveGrepSubCommand::Update { query } => {
                    info!("Updating livegrep"; "query" => Serde(query.clone()));
                    let (mut rx, mut tx) =
                        tokio::io::split(UnixStream::connect(&fzfw_socket).await?);

                    let mode = mode::rg::new().name().to_owned();
                    let args = vec!["--".to_owned(), query];
                    let param = method::LoadParam { mode, args };
                    let resp = send_request(&mut tx, &mut rx, method::Load, param).await?;
                    match resp {
                        Ok(LoadResp { header, items }) => {
                            println!("{}", header);
                            for line in items {
                                println!("{}", line);
                            }
                        }
                        Err(e) => println!("Error: {}", e),
                    }
                }
                LiveGrepSubCommand::GetResult => {
                    info!("Getting livegrep result");
                    let (mut rx, mut tx) =
                        tokio::io::split(UnixStream::connect(&fzfw_socket).await?);
                    let resp = send_request(&mut tx, &mut rx, method::GetLastLoad, ()).await?;
                    match resp {
                        Ok(LoadResp { header, items }) => {
                            println!("{}", header);
                            for line in items {
                                println!("{}", line);
                            }
                        }
                        Err(e) => println!("Error: {}", e),
                    }
                }
            }
            Ok(())
        }
    }
}

// TODO (tx,rx)をまとめる（tokioを隠蔽）
// TODO Result<<M as method::Method>::Response, method::CommonError> みたいにする
// TODO serde_json ではなく rmp_serde を使う？
//      lines() が使えなくなるが、Codec というのを実装すれば代わりに framed() が使えるようになる。
//      参考: https://docs.rs/tokio-util/latest/src/tokio_util/codec/lines_codec.rs.html
//      buf: &mut BytesMut を読んで、成功したらそこまで buf を進めて
//      失敗したら Ok(None) を返せばよい。が、効率よくやる方法がわからん。
//      と思ったら、その部分は rmpv::decode::value::read_value が
//      やってくれるらしい。じゃあ簡単そう。
//
//      これもイメージを掴むのによいかも（古いからそのままでは動かないはず）：
//      https://github.com/little-dude/rmp-rpc/blob/master/src/codec.rs
pub async fn send_request<M: Method>(
    tx: &mut (impl AsyncWriteExt + Unpin),
    rx: &mut (impl AsyncReadExt + Unpin),
    method: M,
    param: <M as method::Method>::Param,
) -> Result<Result<<M as method::Method>::Response, String>, Box<dyn Error>> {
    let req = <M as Method>::request(method, param);
    let req = serde_json::to_string(&req)?;
    let req = format!("{}\n", req);
    tx.write_all(req.as_bytes()).await?;
    let mut rx = BufReader::new(rx).lines();
    let resp = rx.next_line().await?.unwrap();
    let resp = serde_json::from_str(&resp);
    match resp {
        Ok(resp) => Ok(Ok(resp)),
        Err(e) => Ok(Err(e.to_string())),
    }
}
