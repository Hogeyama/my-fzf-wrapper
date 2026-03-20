use std::collections::HashMap;
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

use crate::env::Env;
use crate::logger::Serde;
use crate::method;
use crate::method::ExecuteParam;
use crate::method::LoadParam;
use crate::method::LoadResp;
use crate::method::Method;
use crate::method::PreviewResp;
use crate::mode;
use crate::mode::CallbackMap;
use crate::mode::Mode;
use crate::nvim::NeovimExt;
use crate::state::State;
use crate::utils::fzf;

pub async fn server(env: Env, state: State, listener: UnixListener) -> Result<(), String> {
    let initial_mode_name = env.config.initial_mode.clone();

    // 全モードのコールバックを事前に構築
    let all_modes = Arc::new(env.config.build_all_modes());

    // fzf の --listen 用 Unix ソケットパス
    let fzf_listen_socket = format!("{}.fzf-listen", env.config.socket);

    // 初期モードの fzf 設定を取得
    let (initial_mode, _) = all_modes.get(&initial_mode_name).unwrap_or_else(|| {
        panic!("unknown initial mode: {}", initial_mode_name);
    });
    let fzf_config = initial_mode.fzf_config(mode::FzfArgs {
        myself: env.config.myself.clone(),
        socket: env.config.socket.clone(),
        log_file: env.config.log_file.clone(),
        initial_query: "".to_string(),
        listen_socket: Some(fzf_listen_socket.clone()),
    });

    let env = Arc::new(env);

    let server_state = ServerState {
        fzf: Arc::new(RwLock::new(
            fzf::new(fzf_config)
                .stdout(std::process::Stdio::piped())
                .spawn()
                .expect("Failed to spawn fzf"),
        )),
        fzf_client: Arc::new(fzf::FzfClient::new(&fzf_listen_socket)),
        current_mode_name: Arc::new(RwLock::new(initial_mode_name)),
        all_modes,
        state: Arc::new(RwLock::new(state)),
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

/// ロック順序: fzf → current_mode_name → state
/// all_modes はロック不要 (起動時に構築して不変)
#[derive(Clone)]
struct ServerState {
    fzf: Arc<RwLock<Child>>,
    fzf_client: Arc<fzf::FzfClient>,
    current_mode_name: Arc<RwLock<String>>,
    all_modes: Arc<HashMap<String, (Mode, CallbackMap)>>,
    state: Arc<RwLock<State>>,
}

use tokio::sync::{RwLockReadGuard, RwLockWriteGuard};

impl ServerState {
    /// 現在のモード名を取得し、all_modes から (Mode, CallbackMap) を引く
    fn get_mode_entry<'a>(
        all_modes: &'a HashMap<String, (Mode, CallbackMap)>,
        mode_name: &str,
    ) -> &'a (Mode, CallbackMap) {
        all_modes.get(mode_name).unwrap_or_else(|| {
            panic!("unknown mode: {}", mode_name);
        })
    }

    /// Load/Execute 用: current_mode_name(read), state(write)
    async fn lock_for_load_or_execute(
        &self,
    ) -> (
        RwLockReadGuard<'_, String>,
        RwLockWriteGuard<'_, State>,
    ) {
        let mode_name = self.current_mode_name.read().await;
        let state = self.state.write().await;
        (mode_name, state)
    }

    /// Preview 用: current_mode_name(read)
    async fn lock_for_preview(&self) -> RwLockReadGuard<'_, String> {
        self.current_mode_name.read().await
    }

    /// GetLastLoad 用: state(read)
    async fn lock_for_get_last_load(&self) -> RwLockReadGuard<'_, State> {
        self.state.read().await
    }

    /// ChangeMode 用: fzf(write), current_mode_name(write)
    async fn lock_for_change_mode(
        &self,
    ) -> (
        RwLockWriteGuard<'_, Child>,
        RwLockWriteGuard<'_, String>,
    ) {
        let fzf = self.fzf.write().await;
        let mode_name = self.current_mode_name.write().await;
        (fzf, mode_name)
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

            Some(method::Request::Dispatch { params, method: _ }) => {
                handle_dispatch_request(env, server_state, params, tx).await;
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

    let (mode_name, mut state) = server_state.lock_for_load_or_execute().await;
    let (mode, callbacks) = ServerState::get_mode_entry(&server_state.all_modes, &mode_name);

    let callback = &callbacks
        .load
        .get(&registered_name)
        .unwrap_or_else(|| {
            error!("server: load error";
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
    let mode_name = server_state.lock_for_preview().await;
    let (mode, callbacks) = ServerState::get_mode_entry(&server_state.all_modes, &mode_name);

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

    let (mode_name, mut state) = server_state.lock_for_load_or_execute().await;
    let (mode, callbacks) = ServerState::get_mode_entry(&server_state.all_modes, &mode_name);

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
// NOTE: Phase 3 では既存の kill/spawn 方式を維持。Phase 4 で transform に移行。

async fn handle_change_mode_request(
    env: Arc<Env>,
    server_state: ServerState,
    params: method::ChangeModeParam,
    tx: Arc<Mutex<WriteHalf<UnixStream>>>,
) {
    let method::ChangeModeParam {
        mode: new_mode_name,
        query,
    } = params;

    let (mut fzf, mut current_mode_name) = server_state.lock_for_change_mode().await;

    if let Err(e) = fzf.kill().await {
        error!("server: failed to kill fzf process"; "error" => e.to_string());
    }

    // all_modes から新モードの設定を参照して fzf を再起動
    let (new_mode, _) =
        ServerState::get_mode_entry(&server_state.all_modes, &new_mode_name);
    let listen_socket = server_state
        .fzf_client
        .socket_path()
        .to_string_lossy()
        .to_string();
    let new_fzf_config = new_mode.fzf_config(mode::FzfArgs {
        myself: env.config.myself.clone(),
        socket: env.config.socket.clone(),
        log_file: env.config.log_file.clone(),
        initial_query: query.unwrap_or_default(),
        listen_socket: Some(listen_socket),
    });

    *fzf = fzf::new(new_fzf_config)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to spawn fzf");
    *current_mode_name = new_mode_name;

    let mut tx = tx.lock().await;
    match send_response(method::ChangeMode, &mut *tx, &()).await {
        Ok(()) => trace!("server: change-mode done"),
        Err(e) => error!("server: change-mode error"; "error" => e),
    }
}

// ------------------------------------------------------------------------------
// Dispatch (transform 用)

async fn handle_dispatch_request(
    _env: Arc<Env>,
    server_state: ServerState,
    params: method::DispatchParam,
    tx: Arc<Mutex<WriteHalf<UnixStream>>>,
) {
    let method::DispatchParam { key, query, item } = params;

    info!("server: dispatch"; "key" => &key, "query" => &query, "item" => &item);

    let mode_name = server_state.current_mode_name.read().await;

    // TODO: Phase 4 で実際のディスパッチロジックを実装
    let resp = method::DispatchResp {
        action: format!("# dispatch: mode={}, key={}", *mode_name, key),
    };

    let mut tx = tx.lock().await;
    match send_response(method::Dispatch, &mut *tx, &resp).await {
        Ok(()) => trace!("server: dispatch done"),
        Err(e) => error!("server: dispatch error"; "error" => e),
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
