// Serde
use serde_json::json;

// Tokio
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::net::UnixListener;

use crate::logger::Serde;
use crate::method;
use crate::method::Method;
use crate::method::RunResp;
use crate::types::State;
use crate::utils::clap_parse_from;
use crate::Config;

pub async fn server(
    config: &Config,
    initial_mode: &str,
    initial_state: State,
    listener: UnixListener,
) -> Result<(), String> {
    let mut mode = config.get_mode(initial_mode);
    let mut state = initial_state;
    while let Ok((unix_stream, _addr)) = listener.accept().await {
        trace!("server: new client");
        let (rx, mut tx) = tokio::io::split(unix_stream);
        let mut rx = BufReader::new(rx).lines();

        // request/response loop
        while let Some(line) = rx.next_line().await.map_err(|e| e.to_string())? {
            let req: Option<method::Request> = serde_json::from_str(&line).ok();
            trace!(
                "server: get request";
                "request" => Serde(json!({ "raw": &line, "parsed": &req })),
            );
            match req {
                Some(method::Request::Load { method, params }) => {
                    state.last_load = params.clone(); // save for reload
                    let method::LoadParam {
                        mode: mode_name,
                        args,
                    } = params;
                    mode = config.get_mode(mode_name);
                    let resp = mode.load(&mut state, args).await;
                    match send_response(&mut tx, method, resp).await {
                        Ok(()) => trace!("server: load done"),
                        Err(e) => error!("server: load error"; "error" => e),
                    }
                }
                Some(method::Request::Preview { method, params }) => {
                    let method::PreviewParam { item } = params;
                    let resp = mode.preview(&mut state, item).await;
                    match send_response(&mut tx, method, resp).await {
                        Ok(()) => trace!("server: preview done"),
                        Err(e) => error!("server: preview error"; "error" => e),
                    }
                }
                Some(method::Request::Run { method, params }) => {
                    let method::RunParam { item, args } = params;
                    match clap_parse_from(args) {
                        Ok(opts) => {
                            let resp = mode.run(&mut state, item, opts).await;
                            match send_response(&mut tx, method, resp).await {
                                Ok(()) => trace!("server: run done"),
                                Err(e) => error!("server: run error"; "error" => e),
                            }
                        }
                        Err(e) => {
                            error!("server: clap parse error"; "error" => e.to_string());
                            match send_response(&mut tx, method, RunResp).await {
                                Ok(()) => trace!("server: run done"),
                                Err(e) => error!("server: run error"; "error" => e),
                            }
                        }
                    }
                    info!("server: run done");
                }
                Some(method::Request::Reload { method, params: () }) => {
                    let args = state.last_load.args.clone();
                    let resp = mode.load(&mut state, args).await;
                    match send_response(&mut tx, method, resp).await {
                        Ok(()) => trace!("server: reload done"),
                        Err(e) => error!("server: reload error"; "error" => e),
                    }
                }
                _ => {
                    tx.write_all("\"Unknown request\"".as_bytes())
                        .await
                        .map_err(|e| e.to_string())?;
                }
            }
        }
    }

    Ok(())
}

async fn send_response<M: method::Method, TX: AsyncWriteExt + Unpin>(
    tx: &mut TX,
    _method: M, // 型合わせ用
    resp: <M as Method>::Response,
) -> Result<(), String> {
    let resp = serde_json::to_string(&resp).unwrap() + "\n";
    tx.write_all(resp.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
