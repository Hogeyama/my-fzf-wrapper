use std::sync::Arc;

use futures::stream::AbortHandle;
use futures::stream::Abortable;
use futures::stream::Aborted;
// Serde
use serde_json::json;

// Tokio
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::net::UnixListener;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::logger::Serde;
use crate::method;
use crate::method::Method;
use crate::method::RunResp;
use crate::types::State;
use crate::utils::clap_parse_from;
use crate::Config;

pub async fn server(
    config: Config,
    initial_mode: &str,
    initial_state: State,
    listener: UnixListener,
) -> Result<(), String> {
    let config = Arc::new(config);
    let mode = Arc::new(Mutex::new(config.get_mode(initial_mode)));
    let state = Arc::new(Mutex::new(initial_state));

    let current_load_task: Arc<Mutex<Option<(JoinHandle<Result<(), Aborted>>, AbortHandle)>>> =
        Arc::new(Mutex::new(None));

    while let Ok((unix_stream, _addr)) = listener.accept().await {
        trace!("server: new client");
        let (rx, tx) = tokio::io::split(unix_stream);
        let mut rx = BufReader::new(rx).lines();
        let tx = Arc::new(Mutex::new(tx));

        // request/response loop
        while let Some(line) = rx.next_line().await.map_err(|e| e.to_string())? {
            let req: Option<method::Request> = serde_json::from_str(&line).ok();
            info!(
                "server: get request";
                "request" => Serde(json!({ "raw": &line, "parsed": &req })),
            );
            match req {
                Some(method::Request::Load { method, params }) => {
                    if let Some((_, abort_handle)) = current_load_task.lock().await.take() {
                        abort_handle.abort();
                    }

                    let mode_clone = mode.clone();
                    let state_clone = state.clone();
                    let tx_clone = tx.clone();
                    let new_mode = config.get_mode(params.mode.clone());

                    let (abort_handle, abort_registration) = AbortHandle::new_pair();
                    let handle = tokio::spawn(Abortable::new(
                        async move {
                            let mut mode = mode_clone.lock().await;
                            let mut state = state_clone.lock().await;
                            let mut tx = tx_clone.lock().await;
                            *mode = new_mode;
                            state.last_load = params.clone(); // save for reload
                            let resp = mode.load(&mut state, params.args).await;
                            match send_response(&mut *tx, method, resp).await {
                                Ok(()) => trace!("server: load done"),
                                Err(e) => error!("server: load error"; "error" => e),
                            }
                        },
                        abort_registration,
                    ));
                    *(current_load_task.lock().await) = Some((handle, abort_handle));
                }
                Some(method::Request::Preview { method, params }) => {
                    let mut mode = mode.lock().await;
                    let mut state = state.lock().await;
                    let method::PreviewParam { item } = params;
                    let resp = mode.preview(&mut state, item).await;
                    let mut tx = tx.lock().await;
                    match send_response(&mut *tx, method, resp).await {
                        Ok(()) => trace!("server: preview done"),
                        Err(e) => error!("server: preview error"; "error" => e),
                    }
                }
                Some(method::Request::Run { method, params }) => {
                    let mut mode = mode.lock().await;
                    let mut state = state.lock().await;
                    let method::RunParam { item, args } = params;
                    let mut tx = tx.lock().await;
                    match clap_parse_from(args) {
                        Ok(opts) => {
                            let resp = mode.run(&mut state, item, opts).await;
                            match send_response(&mut *tx, method, resp).await {
                                Ok(()) => trace!("server: run done"),
                                Err(e) => error!("server: run error"; "error" => e),
                            }
                        }
                        Err(e) => {
                            error!("server: clap parse error"; "error" => e.to_string());
                            match send_response(&mut *tx, method, RunResp).await {
                                Ok(()) => trace!("server: run done"),
                                Err(e) => error!("server: run error"; "error" => e),
                            }
                        }
                    }
                    info!("server: run done");
                }
                Some(method::Request::Reload { method, params: () }) => {
                    if let Some((_, abort_handle)) = current_load_task.lock().await.take() {
                        abort_handle.abort();
                    }

                    let mode_clone = mode.clone();
                    let state_clone = state.clone();
                    let tx_clone = tx.clone();

                    let (abort_handle, abort_registration) = AbortHandle::new_pair();
                    let handle = tokio::spawn(Abortable::new(
                        async move {
                            let mut mode = mode_clone.lock().await;
                            let mut state = state_clone.lock().await;
                            let mut tx = tx_clone.lock().await;
                            let args = state.last_load.args.clone();
                            let resp = mode.load(&mut state, args).await;
                            match send_response(&mut *tx, method, resp).await {
                                Ok(()) => trace!("server: reload done"),
                                Err(e) => error!("server: reload error"; "error" => e),
                            }
                        },
                        abort_registration,
                    ));
                    *(current_load_task.lock().await) = Some((handle, abort_handle));
                }
                _ => {
                    let mut tx = tx.lock().await;
                    (&mut *tx)
                        .write_all("\"Unknown request\"".as_bytes())
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
