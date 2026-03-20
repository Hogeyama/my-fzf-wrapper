use std::sync::Arc;
use std::time::Duration;

use futures::stream::AbortHandle;
use futures::stream::Abortable;
use futures::stream::Aborted;
use futures::StreamExt as _;
use futures::TryStreamExt as _;

// Serde
use serde_json::json;

// Tokio
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::io::WriteHalf;
use tokio::net::UnixListener;
use tokio::net::UnixStream;
use tokio::process::Child;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

use crate::logger::Serde;
use crate::method;
use crate::method::ExecuteParam;
use crate::method::LoadParam;
use crate::method::LoadResp;
use crate::method::Method;
use crate::method::PreviewResp;
use crate::mode;
use crate::mode::Mode;
use crate::nvim::NeovimExt;
use crate::state::State;
use crate::utils::fzf;
use crate::env::Env;

pub async fn server(env: Env, state: State, listener: UnixListener) -> Result<(), String> {
    let mode = env.config.get_initial_mode();
    let fzf_config = mode.fzf_config(mode::FzfArgs {
        myself: env.config.myself.clone(),
        socket: env.config.socket.clone(),
        log_file: env.config.log_file.clone(),
        initial_query: "".to_string(),
    });
    let callbacks = mode.callbacks();

    let env = Arc::new(env);

    let server_state = ServerState {
        fzf: Arc::new(RwLock::new(
            fzf::new(fzf_config)
                .stdout(std::process::Stdio::piped())
                .spawn()
                .expect("Failed to spawn fzf"),
        )),
        mode: Arc::new(RwLock::new(mode)),
        state: Arc::new(RwLock::new(state)),
        callbacks: Arc::new(RwLock::new(callbacks)),
    };
    let current_load_task = Arc::new(Mutex::new(None));

    let mut fzf_check_interval = tokio::time::interval(Duration::from_millis(100));
    fzf_check_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            s = listener.accept() => {
                if let Ok((unix_stream, _addr)) = s {
                    handle_one_client(
                        env.clone(),
                        server_state.clone(),
                        current_load_task.clone(),
                        unix_stream,
                    )
                    .await?;
                } else {
                    break;
                }
            }
            _ = fzf_check_interval.tick() => {
                if server_state.check_fzf_alive().await {
                    break; // fzf が死んだのでサーバーも終了
                }
            }
        }
    }

    Ok(())
}

/// ロック順序: fzf → mode → state → callbacks
/// フィールドを非公開にし、ロック取得はメソッド経由で行うことで順序を強制する。
#[derive(Clone)]
struct ServerState {
    fzf: Arc<RwLock<Child>>,
    mode: Arc<RwLock<Mode>>,
    state: Arc<RwLock<State>>,
    callbacks: Arc<RwLock<mode::CallbackMap>>,
}

use tokio::sync::{RwLockReadGuard, RwLockWriteGuard};

impl ServerState {
    /// Load/Execute 用: mode(read), state(write), callbacks(read)
    async fn lock_for_load_or_execute(
        &self,
    ) -> (
        RwLockReadGuard<'_, Mode>,
        RwLockWriteGuard<'_, State>,
        RwLockReadGuard<'_, mode::CallbackMap>,
    ) {
        let mode = self.mode.read().await;
        let state = self.state.write().await;
        let callbacks = self.callbacks.read().await;
        (mode, state, callbacks)
    }

    /// Preview 用: mode(read), callbacks(read)
    async fn lock_for_preview(
        &self,
    ) -> (
        RwLockReadGuard<'_, Mode>,
        RwLockReadGuard<'_, mode::CallbackMap>,
    ) {
        let mode = self.mode.read().await;
        let callbacks = self.callbacks.read().await;
        (mode, callbacks)
    }

    /// GetLastLoad 用: state(read)
    async fn lock_for_get_last_load(&self) -> RwLockReadGuard<'_, State> {
        self.state.read().await
    }

    /// ChangeMode 用: fzf(write), mode(write), callbacks(write)
    async fn lock_for_change_mode(
        &self,
    ) -> (
        RwLockWriteGuard<'_, Child>,
        RwLockWriteGuard<'_, Mode>,
        RwLockWriteGuard<'_, mode::CallbackMap>,
    ) {
        let fzf = self.fzf.write().await;
        let mode = self.mode.write().await;
        let callbacks = self.callbacks.write().await;
        (fzf, mode, callbacks)
    }

    /// fzf プロセスの生存確認用
    async fn check_fzf_alive(&self) -> bool {
        matches!(self.fzf.write().await.try_wait(), Ok(Some(_)))
    }
}

type LoadTask = Arc<Mutex<Option<(JoinHandle<Result<(), Aborted>>, AbortHandle)>>>;

async fn abort_current_load_task(task: &LoadTask) {
    if let Some((_, abort_handle)) = task.lock().await.take() {
        abort_handle.abort();
    }
}

async fn handle_one_client(
    env: Arc<Env>,
    server_state: ServerState,
    current_load_task: LoadTask,
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
            Some(method::Request::Load { params, method: _ }) => {
                abort_current_load_task(&current_load_task).await;
                let (abort_handle, abort_registration) = AbortHandle::new_pair();
                let handle = tokio::spawn(Abortable::new(
                    handle_load_request(env, server_state, params, tx),
                    abort_registration,
                ));
                *(current_load_task.lock().await) = Some((handle, abort_handle));
            }

            Some(method::Request::Preview {
                params,
                preview_window,
                method: _,
            }) => {
                handle_preview_request(env, server_state, params, preview_window, tx).await;
            }

            Some(method::Request::Execute { params, method: _ }) => {
                abort_current_load_task(&current_load_task).await;
                handle_execute_request(env, server_state, params, tx).await;
            }

            Some(method::Request::GetLastLoad {
                params: (),
                method: _,
            }) => {
                abort_current_load_task(&current_load_task).await;
                handle_get_last_load_request(server_state, tx).await;
            }

            Some(method::Request::ChangeMode { params, method: _ }) => {
                abort_current_load_task(&current_load_task).await;
                handle_change_mode_request(env, server_state, params, tx).await;
            }

            Some(method::Request::ChangeDirectory { params, method: _ }) => {
                handle_change_directory_request(env, params, tx).await;
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

// ------------------------------------------------------------------------------
// Load

async fn handle_load_request(
    env: Arc<Env>,
    server_state: ServerState,
    params: LoadParam,
    tx: Arc<Mutex<WriteHalf<UnixStream>>>,
) {
    let LoadParam {
        registered_name,
        query,
        item,
    } = params;

    let (mode, mut state, callbacks) = server_state.lock_for_load_or_execute().await;

    let callback = &callbacks
        .load
        .get(&registered_name)
        .unwrap_or_else(|| {
            error!("server: execute error";
                "error" => "unknown callback",
                "registered_name" => registered_name
            );
            panic!("unknown callback");
        })
        .callback;

    state.last_load_resp = {
        let stream = callback(
            mode.mode_def.as_ref(),
            &env,
            &mut state,
            query,
            item.unwrap_or_default(),
        );
        send_load_stream(stream, tx).await
    };
}

async fn send_load_stream(
    stream: mode::LoadStream<'_>,
    tx: Arc<Mutex<WriteHalf<UnixStream>>>,
) -> Option<LoadResp> {
    let r = stream
        .map(|resp| resp.unwrap_or_else(LoadResp::error))
        .map(Ok::<_, anyhow::Error>) // try_foldを使うために持ち上げる
        .try_fold((None, vec![]), |(mut header, mut items), resp| async {
            let mut tx = tx.lock().await;
            match send_response(method::Load, &mut *tx, &resp).await {
                Ok(()) => {
                    trace!("server: load done");
                    header = header.or(resp.header);
                    items.extend(resp.items);
                    Ok((header, items))
                }
                Err(e) => {
                    error!("server: load error"; "error" => &e);
                    Err(anyhow::anyhow!(e))
                }
            }
        })
        .await;

    match r {
        Ok((header, items)) => Some(LoadResp {
            header,
            items,
            is_last: true,
        }),
        Err(_) => None,
    }
}

// ------------------------------------------------------------------------------
// Preview

async fn handle_preview_request(
    env: Arc<Env>,
    server_state: ServerState,
    params: method::PreviewParam,
    preview_window: fzf::PreviewWindow,
    tx: Arc<Mutex<WriteHalf<UnixStream>>>,
) {
    let (mode, callbacks) = server_state.lock_for_preview().await;

    let callback = &callbacks
        .preview
        .get("default")
        .unwrap_or_else(|| {
            panic!("unknown callback");
        })
        .callback;

    let resp = callback(
        mode.mode_def.as_ref(),
        &env,
        &preview_window,
        params.item,
    )
    .await
    .unwrap_or_else(PreviewResp::error);

    let mut tx = tx.lock().await;
    match send_response(method::Preview, &mut *tx, &resp).await {
        Ok(()) => trace!("server: preview done"),
        Err(e) => error!("server: preview error"; "error" => e),
    }
}

// ------------------------------------------------------------------------------
// Execute

async fn handle_execute_request(
    env: Arc<Env>,
    server_state: ServerState,
    params: method::ExecuteParam,
    tx: Arc<Mutex<WriteHalf<UnixStream>>>,
) {
    let ExecuteParam {
        registered_name,
        query,
        item,
    } = params;

    let (mode, mut state, callbacks) = server_state.lock_for_load_or_execute().await;

    let callback = &callbacks
        .execute
        .get(&registered_name)
        .unwrap_or_else(|| {
            error!("server: execute error";
                "error" => "unknown callback",
                "registered_name" => registered_name
            );
            panic!("unknown callback");
        })
        .callback;

    match callback(mode.mode_def.as_ref(), &env, &mut state, query, item).await {
        Ok(_) => {}
        Err(e) => error!("server: execute error"; "error" => e.to_string()),
    }

    let mut tx = tx.lock().await;
    match send_response(method::Execute, &mut *tx, &()).await {
        Ok(()) => info!("server: execute done"),
        Err(e) => error!("server: execute error"; "error" => e),
    }
}

// ------------------------------------------------------------------------------
// GetLastLoad

async fn handle_get_last_load_request(
    server_state: ServerState,
    tx: Arc<Mutex<WriteHalf<UnixStream>>>,
) {
    let state = server_state.lock_for_get_last_load().await;

    let mut tx = tx.lock().await;
    let resp = match &state.last_load_resp {
        Some(resp) => resp.clone(),
        None => method::LoadResp {
            header: Some("".to_string()),
            items: vec![],
            is_last: true,
        },
    };
    match send_response(method::GetLastLoad, &mut *tx, &resp).await {
        Ok(()) => trace!("server: get-last-load done"),
        Err(e) => error!("server: get-last-load error"; "error" => e),
    }
}

// ------------------------------------------------------------------------------
// ChangeMode

async fn handle_change_mode_request(
    env: Arc<Env>,
    server_state: ServerState,
    params: method::ChangeModeParam,
    tx: Arc<Mutex<WriteHalf<UnixStream>>>,
) {
    let method::ChangeModeParam {
        mode: new_mode,
        query,
    } = params;

    let (mut fzf, mut mode, mut callbacks) = server_state.lock_for_change_mode().await;

    if let Err(e) = fzf.kill().await {
        error!("server: failed to kill fzf process"; "error" => e.to_string());
    }

    let new_mode = env.config.get_mode(new_mode);
    let new_callback_map = new_mode.callbacks();
    let new_fzf_config = new_mode.fzf_config(mode::FzfArgs {
        myself: env.config.myself.clone(),
        socket: env.config.socket.clone(),
        log_file: env.config.log_file.clone(),
        initial_query: query.unwrap_or_default(),
    });

    *fzf = fzf::new(new_fzf_config)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to spawn fzf");
    *mode = new_mode;
    *callbacks = new_callback_map;

    let mut tx = tx.lock().await;
    match send_response(method::ChangeMode, &mut *tx, &()).await {
        Ok(()) => trace!("server: change-mode done"),
        Err(e) => error!("server: change-mode error"; "error" => e),
    }
}

// ------------------------------------------------------------------------------
// ChangeDirectory

async fn handle_change_directory_request(
    env: Arc<Env>,
    params: method::ChangeDirectoryParam,
    tx: Arc<Mutex<WriteHalf<UnixStream>>>,
) {
    let dir = match params {
        method::ChangeDirectoryParam::ToParent => {
            let mut dir = std::env::current_dir().unwrap();
            dir.pop();
            Ok(dir)
        }
        method::ChangeDirectoryParam::ToLastFileDir => env
            .nvim
            .last_opened_file()
            .await
            .map_err(|e| e.to_string())
            .and_then(|path| {
                let path = std::fs::canonicalize(path).unwrap();
                path.parent()
                    .ok_or("no parent dir".to_string())
                    .map(|p| p.to_owned())
            }),
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
    match send_response(method::ChangeDirectory, &mut *tx, &()).await {
        Ok(()) => trace!("server: change-directory done"),
        Err(e) => {
            error!("server: change-directory error"; "error" => e);
        }
    }
}

// ------------------------------------------------------------------------------
// Util

async fn send_response<M: method::Method, TX: AsyncWriteExt + Unpin>(
    _method: M, // 型合わせ用
    tx: &mut TX,
    resp: &<M as Method>::Response,
) -> std::io::Result<()> {
    let resp = serde_json::to_string(&resp).unwrap() + "\n";
    tx.write_all(resp.as_bytes()).await?;
    Ok(())
}
