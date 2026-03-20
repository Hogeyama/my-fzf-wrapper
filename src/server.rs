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

use crate::config::ModeEntry;
use crate::env::Env;
use crate::logger::Serde;
use crate::method;
use crate::method::ExecuteParam;
use crate::method::LoadParam;
use crate::method::LoadResp;
use crate::method::Method;
use crate::method::PreviewResp;
use crate::mode;
use crate::nvim::NeovimExt;
use crate::state::State;
use crate::utils::fzf;

pub async fn server(env: Env, state: State, listener: UnixListener) -> Result<(), String> {
    let initial_mode_name = env.config.initial_mode.clone();

    // 全モードのコールバックと rendered_bindings を事前に構築
    let all_modes = Arc::new(env.config.build_all_modes());

    // fzf の --listen 用 Unix ソケットパス
    let fzf_listen_socket = format!("unix:{}.fzf-listen", env.config.socket);

    // 統合バインディングを構築 (全モードの全キーを transform dispatch に)
    let unified_bindings = env.config.build_unified_bindings(&all_modes);

    // 初期モードの prompt を取得
    let initial_entry = all_modes.get(&initial_mode_name).unwrap_or_else(|| {
        panic!("unknown initial mode: {}", initial_mode_name);
    });
    let initial_prompt = initial_entry.mode.mode_def.fzf_prompt();

    // 統合 fzf 設定で起動 (--no-sort + --multi を付与)
    let fzf_config = fzf::Config {
        myself: env.config.myself.clone(),
        socket: env.config.socket.clone(),
        log_file: env.config.log_file.clone(),
        load: vec![
            "load".to_string(),
            "default".to_string(),
            "".to_string(), // query
            "".to_string(), // item
        ],
        initial_prompt,
        initial_query: "".to_string(),
        bindings: unified_bindings,
        extra_opts: vec![
            "--no-sort".to_string(),
            "--multi".to_string(),
        ],
        listen_socket: Some(fzf_listen_socket.clone()),
    };

    let env = Arc::new(env);

    // 初期モードの sort 状態
    let initial_sort = initial_entry.mode.mode_def.wants_sort();

    let server_state = ServerState {
        fzf: Arc::new(RwLock::new(
            fzf::new(fzf_config)
                .stdout(std::process::Stdio::piped())
                .spawn()
                .expect("Failed to spawn fzf"),
        )),
        fzf_client: Arc::new(fzf::FzfClient::new(
            fzf_listen_socket.strip_prefix("unix:").unwrap_or(&fzf_listen_socket),
        )),
        current_mode_name: Arc::new(RwLock::new(initial_mode_name)),
        all_modes,
        state: Arc::new(RwLock::new(state)),
        sort_enabled: Arc::new(RwLock::new(initial_sort)),
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

/// ロック順序: fzf → current_mode_name → state → sort_enabled
/// all_modes はロック不要 (起動時に構築して不変)
#[derive(Clone)]
struct ServerState {
    fzf: Arc<RwLock<Child>>,
    fzf_client: Arc<fzf::FzfClient>,
    current_mode_name: Arc<RwLock<String>>,
    all_modes: Arc<HashMap<String, ModeEntry>>,
    state: Arc<RwLock<State>>,
    sort_enabled: Arc<RwLock<bool>>,
}

use tokio::sync::{RwLockReadGuard, RwLockWriteGuard};

impl ServerState {
    fn get_mode_entry<'a>(
        all_modes: &'a HashMap<String, ModeEntry>,
        mode_name: &str,
    ) -> &'a ModeEntry {
        all_modes.get(mode_name).unwrap_or_else(|| {
            panic!("unknown mode: {}", mode_name);
        })
    }

    /// Load/Execute 用: current_mode_name(read), state(write)
    async fn lock_for_load_or_execute(
        &self,
    ) -> (RwLockReadGuard<'_, String>, RwLockWriteGuard<'_, State>) {
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
                // 後方互換: 旧 change-mode リクエストも dispatch 経由で処理
                abort_current_load_task(&current_load_task).await;
                let dispatch_params = method::DispatchParam {
                    key: format!("change-mode:{}", params.mode),
                    query: params.query.unwrap_or_default(),
                    item: String::new(),
                };
                handle_dispatch_request(env, server_state, dispatch_params, tx).await;
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
    let entry = ServerState::get_mode_entry(&server_state.all_modes, &mode_name);

    let callback = &entry
        .callbacks
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
            entry.mode.mode_def.as_ref(),
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
        .map(Ok::<_, anyhow::Error>)
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
    let entry = ServerState::get_mode_entry(&server_state.all_modes, &mode_name);

    let callback = &entry
        .callbacks
        .preview
        .get("default")
        .unwrap_or_else(|| {
            panic!("unknown callback");
        })
        .callback;

    let resp = callback(
        entry.mode.mode_def.as_ref(),
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
    let entry = ServerState::get_mode_entry(&server_state.all_modes, &mode_name);

    let callback = &entry
        .callbacks
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

    match callback(entry.mode.mode_def.as_ref(), &env, &mut state, query, item).await {
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
// Dispatch (transform 用: モード切替 + モード依存キーのディスパッチ)

async fn handle_dispatch_request(
    _env: Arc<Env>,
    server_state: ServerState,
    params: method::DispatchParam,
    tx: Arc<Mutex<WriteHalf<UnixStream>>>,
) {
    let method::DispatchParam { key, query, item } = params;

    info!("server: dispatch"; "key" => &key, "query" => &query, "item" => &item);

    let action = if let Some(new_mode_name) = key.strip_prefix("change-mode:") {
        dispatch_change_mode(&server_state, new_mode_name, &key).await
    } else {
        dispatch_mode_key(&server_state, &key).await
    };

    let resp = method::DispatchResp { action };

    let mut tx = tx.lock().await;
    match send_response(method::Dispatch, &mut *tx, &resp).await {
        Ok(()) => trace!("server: dispatch done"),
        Err(e) => error!("server: dispatch error"; "error" => e),
    }
}

/// モード切替の dispatch: current_mode_name 更新 + fzf アクション文字列を生成
async fn dispatch_change_mode(
    server_state: &ServerState,
    new_mode_name: &str,
    key: &str,
) -> String {
    let entry = match server_state.all_modes.get(new_mode_name) {
        Some(e) => e,
        None => {
            error!("server: dispatch unknown mode"; "key" => key);
            return String::new();
        }
    };

    // current_mode_name を更新
    {
        let mut mode_name = server_state.current_mode_name.write().await;
        *mode_name = new_mode_name.to_string();
    }

    let new_mode_def = entry.mode.mode_def.as_ref();
    let mut actions: Vec<String> = Vec::new();

    // reload: 引数なしで FZF_DEFAULT_COMMAND を再実行
    // (query は fzf の内部フィルタで処理される。livegrep は change イベントで reload)
    actions.push("reload".to_string());
    actions.push(format!("change-prompt({})", new_mode_def.fzf_prompt()));

    // livegrep への切替は query を保持 (ctrl-g は keep_query=true)
    if !key.contains("livegrep") {
        actions.push("clear-query".to_string());
    }

    // sort 状態の管理
    let new_wants_sort = new_mode_def.wants_sort();
    {
        let mut sort_enabled = server_state.sort_enabled.write().await;
        if *sort_enabled != new_wants_sort {
            actions.push("toggle-sort".to_string());
            *sort_enabled = new_wants_sort;
        }
    }

    // search の有効/無効
    let has_disable_search = new_mode_def
        .mode_enter_actions()
        .iter()
        .any(|a| matches!(a, fzf::Action::DisableSearch));
    if has_disable_search {
        actions.push("disable-search".to_string());
    } else {
        actions.push("enable-search".to_string());
    }

    // preview-window のリセット/変更
    let custom_preview = new_mode_def
        .mode_enter_actions()
        .into_iter()
        .find_map(|a| match a {
            fzf::Action::ChangePreviewWindow(spec) => Some(spec),
            _ => None,
        });
    actions.push(format!(
        "change-preview-window({})",
        custom_preview.as_deref().unwrap_or("right:50%:noborder")
    ));

    // deselect-all (multi モードのリセット)
    actions.push("deselect-all".to_string());

    actions.join("+")
}

/// モード依存キーの dispatch: 現在モードの rendered_bindings から返す
async fn dispatch_mode_key(server_state: &ServerState, key: &str) -> String {
    let mode_name = server_state.current_mode_name.read().await;
    let entry = ServerState::get_mode_entry(&server_state.all_modes, &mode_name);

    match entry.rendered_bindings.get(key) {
        Some(action) => action.clone(),
        None => {
            // このモードにはこのキーのバインディングがない → no-op
            trace!("server: dispatch no binding"; "mode" => &*mode_name, "key" => key);
            String::new()
        }
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
