use std::ops::DerefMut;
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
use crate::method::ExecuteParam;
use crate::method::LoadParam;
use crate::method::LoadResp;
use crate::method::Method;
use crate::method::PreviewResp;
use crate::mode;
use crate::mode::Mode;
use crate::nvim;
use crate::state::State;
use crate::Config;

pub async fn server(
    myself: String,
    config: Config,
    state: State,
    socket: String,
    log_file: String,
    initial_mode: String,
    listener: UnixListener,
) -> Result<(), String> {
    let mode = config.get_mode(initial_mode);
    let fzf_config = mode.fzf_config(mode::FzfArgs {
        myself: myself.clone(),
        socket: socket.clone(),
        log_file: log_file.clone(),
        initial_query: "".to_string(),
    });
    let callbacks = mode.callbacks();

    let config = Arc::new(config);

    let server_state = Arc::new(Mutex::new(ServerState {
        fzf: fzf::new(fzf_config)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("Failed to spawn fzf"),
        mode,
        state,
        callbacks,
    }));
    let current_load_task = Arc::new(Mutex::new(None));

    loop {
        tokio::select! {
            s = listener.accept() => {
                if let Ok((unix_stream, _addr)) = s {
                    handle_one_client(
                        myself.clone(),
                        config.clone(),
                        server_state.clone(),
                        current_load_task.clone(),
                        socket.clone(),
                        log_file.clone(),
                        unix_stream,
                    )
                    .await?;
                } else {
                    break;
                }
            }
            s = async {
                sleep(Duration::from_millis(100)).await;
                server_state.lock().await.fzf.try_wait()
            } => {
                if let Ok(Some(_)) = s {
                    break; // fzf が死んだのでサーバーも終了
                }
            }
        }
    }

    Ok(())
}

struct ServerState {
    fzf: Child,
    mode: Mode,
    state: State,
    callbacks: mode::CallbackMap,
}

type MutexServerState = Arc<Mutex<ServerState>>;

async fn handle_one_client(
    myself: String,
    config: Arc<Config>,
    server_state: MutexServerState,
    current_load_task: Arc<Mutex<Option<(JoinHandle<Result<(), Aborted>>, AbortHandle)>>>,
    socket: String,
    log_file: String,
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

                let (abort_handle, abort_registration) = AbortHandle::new_pair();
                let handle = tokio::spawn(Abortable::new(
                    async move {
                        let mut s = server_state.lock().await;
                        let s = s.deref_mut();

                        let LoadParam {
                            registered_name,
                            query,
                            item,
                        } = params;

                        let ref mut callback = s
                            .callbacks
                            .load
                            .get_mut(&registered_name)
                            .unwrap_or_else(|| {
                                error!("server: execute error";
                                    "error" => "unknown callback",
                                    "registered_name" => registered_name
                                );
                                panic!("unknown callback");
                            })
                            .callback;

                        let resp = callback(s.mode.mode_def.as_ref(), &mut s.state, query, item)
                            .await
                            .unwrap_or_else(LoadResp::error);

                        s.state.last_load_resp = Some(resp.clone());
                        let mut tx = tx.lock().await;
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
                let mut s = server_state.lock().await;
                let s = s.deref_mut();
                let ref mut callback = s
                    .callbacks
                    .preview
                    .get_mut("default")
                    .unwrap_or_else(|| {
                        panic!("unknown callback");
                    })
                    .callback;
                let resp = callback(s.mode.mode_def.as_ref(), &mut s.state, params.item)
                    .await
                    .unwrap_or_else(PreviewResp::error);
                let mut tx = tx.lock().await;
                match send_response(&mut *tx, method, resp).await {
                    Ok(()) => trace!("server: preview done"),
                    Err(e) => error!("server: preview error"; "error" => e),
                }
            }

            Some(method::Request::Execute { method, params }) => {
                let mut s = server_state.lock().await;
                let s = s.deref_mut();
                let ExecuteParam {
                    registered_name,
                    query,
                    item,
                } = params;

                let ref mut callback = s
                    .callbacks
                    .execute
                    .get_mut(&registered_name)
                    .unwrap_or_else(|| {
                        error!("server: execute error";
                            "error" => "unknown callback",
                            "registered_name" => registered_name
                        );
                        panic!("unknown callback");
                    })
                    .callback;

                match callback(s.mode.mode_def.as_ref(), &mut s.state, query, item).await {
                    Ok(_) => {}
                    Err(e) => error!("server: execute error"; "error" => e),
                }

                let mut tx = tx.lock().await;
                match send_response(&mut *tx, method, ()).await {
                    Ok(()) => info!("server: execute done"),
                    Err(e) => error!("server: execute error"; "error" => e),
                }
            }

            Some(method::Request::GetLastLoad { method, params: () }) => {
                let s = server_state.lock().await;
                let mut tx = tx.lock().await;
                let resp = match &s.state.last_load_resp {
                    Some(resp) => resp.clone(),
                    None => method::LoadResp {
                        header: "".to_string(),
                        items: vec![],
                    },
                };
                match send_response(&mut *tx, method, resp).await {
                    Ok(()) => trace!("server: get-last-load done"),
                    Err(e) => error!("server: get-last-load error"; "error" => e),
                }
            }

            Some(method::Request::ChangeMode { method, params }) => {
                let method::ChangeModeParam {
                    mode: new_mode,
                    query,
                } = params;
                let mut s = server_state.lock().await;
                unsafe { libc::kill(s.fzf.id().unwrap() as i32, libc::SIGTERM) };

                let new_mode = config.get_mode(new_mode);
                let new_callback_map = new_mode.callbacks();
                let new_fzf_config = new_mode.fzf_config(mode::FzfArgs {
                    myself,
                    socket,
                    log_file,
                    initial_query: query.unwrap_or_default(),
                });

                s.fzf = fzf::new(new_fzf_config)
                    .stdout(std::process::Stdio::piped())
                    .spawn()
                    .expect("Failed to spawn fzf");
                s.mode = new_mode;
                s.callbacks = new_callback_map;

                let mut tx = tx.lock().await;
                match send_response(&mut *tx, method, ()).await {
                    Ok(()) => trace!("server: change-mode done"),
                    Err(e) => error!("server: change-mode error"; "error" => e),
                }
            }

            Some(method::Request::ChangeDirectory { method, params }) => {
                let s = server_state.lock().await;
                let dir = match params {
                    method::ChangeDirectoryParam::ToParent => {
                        let mut dir = std::env::current_dir().unwrap();
                        dir.pop();
                        Ok(dir)
                    }
                    method::ChangeDirectoryParam::ToLastFileDir => {
                        nvim::last_opened_file(&s.state.nvim)
                            .await
                            .map_err(|e| e.to_string())
                            .and_then(|path| {
                                let path = std::fs::canonicalize(path).unwrap();
                                path.parent()
                                    .ok_or("no parent dir".to_string())
                                    .map(|p| p.to_owned())
                            })
                    }
                    method::ChangeDirectoryParam::To(path) => std::fs::canonicalize(path)
                        .map_err(|e| e.to_string())
                        .and_then(|path| match std::fs::metadata(&path) {
                            Ok(metadata) if metadata.is_dir() => Ok(path.to_owned()),
                            Ok(metadata) if metadata.is_file() => path
                                .parent()
                                .ok_or("no parent dir".to_string())
                                .map(|p| p.to_owned()),
                            _ => Err(format!("path does not exists: {:?}", path)),
                        }),
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
            _ => {
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
