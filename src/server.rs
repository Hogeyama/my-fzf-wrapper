use std::sync::Arc;
use std::time::Duration;

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
use tokio::net::UnixStream;
use tokio::process::Child;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::sleep;

use crate::external_command::fzf;
use crate::logger::Serde;
use crate::method;
use crate::method::LoadResp;
use crate::method::Method;
use crate::method::PreviewResp;
use crate::method::RunResp;
use crate::nvim;
use crate::types::FzfConfig;
use crate::types::State;
use crate::utils::clap_parse_from;
use crate::Config;

pub async fn server(
    myself: String,
    config: Config,
    state: State,
    socket: String,
    log_file: String,
    listener: UnixListener,
) -> Result<(), String> {
    let fzf = Arc::new(Mutex::new(
        fzf::new(state.mode.as_ref().unwrap().fzf_config(FzfConfig {
            myself: myself.clone(),
            socket: socket.clone(),
            log_file: log_file.clone(),
            args: vec![],
            initial_query: None,
        }))
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to spawn fzf"),
    ));
    let config = Arc::new(config);
    let state = Arc::new(Mutex::new(state));
    let current_load_task = Arc::new(Mutex::new(None));

    loop {
        tokio::select! {
            s = listener.accept() => {
                if let Ok((unix_stream, _addr)) = s {
                    handle_one_client(
                        myself.clone(),
                        config.clone(),
                        state.clone(),
                        socket.clone(),
                        log_file.clone(),
                        fzf.clone(),
                        current_load_task.clone(),
                        unix_stream,
                    )
                    .await?;
                } else {
                    break;
                }
            }
            s = async {
                sleep(Duration::from_millis(100)).await;
                fzf.lock().await.try_wait()
            } => {
                if let Ok(Some(_)) = s {
                    break; // fzf が死んだのでサーバーも終了
                }
            }
        }
    }

    Ok(())
}

async fn handle_one_client(
    myself: String,
    config: Arc<Config>,
    state: Arc<Mutex<State>>,
    socket: String,
    log_file: String,
    fzf: Arc<Mutex<Child>>,
    current_load_task: Arc<Mutex<Option<(JoinHandle<Result<(), Aborted>>, AbortHandle)>>>,
    unix_stream: UnixStream,
) -> Result<(), String> {
    let (rx, tx) = tokio::io::split(unix_stream);
    let mut rx = BufReader::new(rx).lines();
    let tx = Arc::new(Mutex::new(tx));

    if let Some(line) = rx.next_line().await.map_err(|e| e.to_string())? {
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

                let state_clone = state.clone();
                let tx_clone = tx.clone();
                let new_mode = config.get_mode(params.mode.clone());

                let (abort_handle, abort_registration) = AbortHandle::new_pair();
                let handle = tokio::spawn(Abortable::new(
                    async move {
                        let mut state = state_clone.lock().await;
                        let mut tx = tx_clone.lock().await;
                        state.mode = None;
                        let resp = new_mode
                            .load(&mut state, params.clone().args)
                            .await
                            .unwrap_or_else(LoadResp::error);
                        if state.mode.is_none() {
                            // load が state.mode をセットする可能性があるため、
                            // そうでない場合のみここでセットする。
                            // 他のメソッドでも同様。
                            state.mode = Some(new_mode);
                            state.last_load_param = params.clone();
                            state.last_load_resp = Some(resp.clone());
                        }
                        match send_response(&mut *tx, method, resp).await {
                            Ok(()) => trace!("server: reload done"),
                            Err(e) => error!("server: reload error"; "error" => e),
                        }
                    },
                    abort_registration,
                ));
                *(current_load_task.lock().await) = Some((handle, abort_handle));
            }
            Some(method::Request::Preview { method, params }) => {
                let method::PreviewParam { item } = params;
                let mut state = state.lock().await;
                let mode = std::mem::take(&mut state.mode).unwrap();
                let resp = mode
                    .preview(&mut state, item)
                    .await
                    .unwrap_or_else(PreviewResp::error);
                if state.mode.is_none() {
                    state.mode = Some(mode);
                }
                let mut tx = tx.lock().await;
                match send_response(&mut *tx, method, resp).await {
                    Ok(()) => trace!("server: preview done"),
                    Err(e) => error!("server: preview error"; "error" => e),
                }
            }
            Some(method::Request::Run { method, params }) => {
                let mut state = state.lock().await;
                let method::RunParam { item, args } = params;
                let mut tx = tx.lock().await;
                match clap_parse_from(args) {
                    Ok(opts) => {
                        let mode = std::mem::take(&mut state.mode).unwrap();
                        let resp = mode
                            .run(&mut state, item.clone(), opts)
                            .await
                            .unwrap_or_else(|e| {
                                error!("server: run error"; "error" => e.to_string());
                                RunResp
                            });
                        if state.mode.is_none() {
                            state.mode = Some(mode);
                        }
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

                let state_clone = state.clone();
                let tx_clone = tx.clone();

                let (abort_handle, abort_registration) = AbortHandle::new_pair();
                let handle = tokio::spawn(Abortable::new(
                    async move {
                        let mut state = state_clone.lock().await;
                        let mode = std::mem::take(&mut state.mode).unwrap();
                        let mut tx = tx_clone.lock().await;
                        let args = state.last_load_param.args.clone();
                        let resp = mode
                            .load(&mut state, args)
                            .await
                            .unwrap_or_else(LoadResp::error);
                        if state.mode.is_none() {
                            state.mode = Some(mode);
                            state.last_load_resp = Some(resp.clone());
                        }
                        match send_response(&mut *tx, method, resp).await {
                            Ok(()) => trace!("server: reload done"),
                            Err(e) => error!("server: reload error"; "error" => e),
                        }
                    },
                    abort_registration,
                ));
                *(current_load_task.lock().await) = Some((handle, abort_handle));
            }
            Some(method::Request::GetLastLoad { method, params: () }) => {
                if let Some((_, abort_handle)) = current_load_task.lock().await.take() {
                    abort_handle.abort();
                }
                let state = state.lock().await;
                let mut tx = tx.lock().await;
                let resp = match &state.last_load_resp {
                    Some(resp) => resp.clone(),
                    None => method::LoadResp {
                        header: "".to_string(),
                        items: vec![],
                    },
                };
                match send_response(&mut *tx, method, resp).await {
                    Ok(()) => trace!("server: reload done"),
                    Err(e) => error!("server: reload error"; "error" => e),
                }
            }
            Some(method::Request::ChangeMode { method, params }) => {
                let method::ChangeModeParam { mode, query, args } = params;

                // 実行中のロードをキャンセル
                if let Some((_, abort_handle)) = current_load_task.lock().await.take() {
                    abort_handle.abort();
                }

                // fzf を殺す前にロックをとっておく。
                // そうしないと上の方の select! でサーバーが死ぬ。
                debug!("server: lock fzf");
                let mut fzf = fzf.lock().await;
                unsafe { libc::kill(fzf.id().unwrap() as i32, libc::SIGTERM) };

                // 指定されたモードでfzfを再起動
                // param が None の場合は fzf の output をそのまま使う
                let selected_mode = config.get_mode(mode);
                debug!("server: spawn fzf: start");
                *fzf = fzf::new(selected_mode.fzf_config(FzfConfig {
                    myself,
                    socket,
                    log_file,
                    args,
                    initial_query: query,
                }))
                .stdout(std::process::Stdio::piped())
                .spawn()
                .expect("Failed to spawn fzf");
                debug!("server: spawn fzf: end");
                (*state.lock().await).mode = Some(selected_mode);
                debug!("server: change-mode done");

                let mut tx = tx.lock().await;
                match send_response(&mut *tx, method, ()).await {
                    Ok(()) => trace!("server: change-mode done"),
                    Err(e) => error!("server: change-mode error"; "error" => e),
                }
            }
            Some(method::Request::ChangeDirectory { method, params }) => {
                let dir = match params {
                    method::ChangeDirectoryParam::ToParent => {
                        let mut dir = std::env::current_dir().unwrap();
                        dir.pop();
                        Ok(dir)
                    }
                    method::ChangeDirectoryParam::ToLastFileDir => {
                        let nvim = &state.lock().await.nvim;
                        nvim::last_opened_file(nvim)
                            .await
                            .map_err(|e| e.to_string())
                            .and_then(|path| {
                                let path = std::fs::canonicalize(path).unwrap();
                                path.parent()
                                    .ok_or("no parent dir".to_string())
                                    .map(|p| p.to_owned())
                            })
                    }
                    method::ChangeDirectoryParam::To(path) => {
                        let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
                        match std::fs::metadata(&path) {
                            Ok(metadata) if metadata.is_dir() => Ok(path.to_owned()),
                            Ok(metadata) if metadata.is_file() => path
                                .parent()
                                .ok_or("no parent dir".to_string())
                                .map(|p| p.to_owned()),
                            _ => Err(format!("path does not exists: {:?}", path)),
                        }
                    }
                };

                match dir {
                    Ok(dir) => {
                        std::env::set_current_dir(dir).ok();
                    }
                    Err(e) => error!("server: change-directory error"; "error" => e),
                }

                let mut tx = tx.lock().await;
                match send_response(&mut *tx, method, ()).await {
                    Ok(()) => trace!("server: change-mode done"),
                    Err(e) => error!("server: change-mode error"; "error" => e),
                }
            }
            None => {
                let mut tx = tx.lock().await;
                (*tx)
                    .write_all("\"Unknown request\"".as_bytes())
                    .await
                    .map_err(|e| e.to_string())?;
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
