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

use crate::config::Config;
use crate::config::ModeEntry;
use crate::env::Env;
use crate::env::ModeInfo;
use crate::logger::Serde;
use crate::method;
use crate::method::ExecuteParam;
use crate::method::LoadParam;
use crate::method::LoadResp;
use crate::method::Method;
use crate::method::PreviewResp;
use crate::mode;
use crate::nvim::Neovim;
use crate::state::State;
use crate::utils::fzf;

pub async fn server(config: Config, nvim: Neovim, listener: UnixListener) -> Result<(), String> {
    let initial_mode_name = config.initial_mode.clone();

    // 全モードのコールバックと rendered_bindings を事前に構築
    let all_modes = Arc::new(config.build_all_modes());

    // fzf の --listen 用 Unix ソケットパス
    let fzf_listen_socket = config.socket.replace(".sock", "-fzf-listen.sock");

    // 統合バインディングを構築 (全モードの全キーを transform dispatch に)
    let unified_bindings = config.build_unified_bindings(&all_modes);

    // 初期モードの prompt を取得
    let initial_entry = all_modes.get(&initial_mode_name).unwrap_or_else(|| {
        panic!("unknown initial mode: {}", initial_mode_name);
    });
    let initial_prompt = initial_entry.mode.mode_def.fzf_prompt();
    let initial_sort = initial_entry.mode.mode_def.wants_sort();

    // モード切替用メタデータを事前計算
    let mode_infos: HashMap<String, ModeInfo> = all_modes
        .iter()
        .map(|(name, entry)| {
            let def = entry.mode.mode_def.as_ref();
            let enter_actions = def.mode_enter_actions();
            (
                name.clone(),
                ModeInfo {
                    prompt: def.fzf_prompt(),
                    wants_sort: def.wants_sort(),
                    disable_search: enter_actions
                        .iter()
                        .any(|a| matches!(a, fzf::Action::DisableSearch)),
                    custom_preview_window: enter_actions.into_iter().find_map(|a| match a {
                        fzf::Action::ChangePreviewWindow(spec) => Some(spec),
                        _ => None,
                    }),
                },
            )
        })
        .collect();

    // FzfClient を作成
    let fzf_client = Arc::new(fzf::FzfClient::new(
        &fzf_listen_socket,
        config.myself.clone(),
    ));

    // 統合 fzf 設定で起動 (--no-sort + --multi を付与)
    let fzf_config = fzf::Config {
        myself: config.myself.clone(),
        socket: config.socket.clone(),
        log_file: config.log_file.clone(),
        load: vec![
            "load".to_string(),
            "default".to_string(),
            "".to_string(), // query
            "".to_string(), // item
        ],
        initial_prompt,
        initial_query: "".to_string(),
        bindings: unified_bindings,
        extra_opts: vec!["--no-sort".to_string(), "--multi".to_string()],
        listen_socket: Some(fzf_listen_socket),
    };

    // Env を構築
    let env = Arc::new(Env {
        config,
        nvim,
        fzf_client: fzf_client.clone(),
        mode_infos: Arc::new(mode_infos),
    });

    // State を構築
    let state = State::new(initial_mode_name, initial_sort);

    let server_state = ServerState {
        fzf: Arc::new(RwLock::new(
            fzf::new(fzf_config)
                .stdout(std::process::Stdio::piped())
                .spawn()
                .expect("Failed to spawn fzf"),
        )),
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

/// ロック順序: fzf → state
/// all_modes はロック不要 (起動時に構築して不変)
#[derive(Clone)]
struct ServerState {
    fzf: Arc<RwLock<Child>>,
    all_modes: Arc<HashMap<String, ModeEntry>>,
    state: Arc<RwLock<State>>,
}

use tokio::sync::RwLockWriteGuard;

impl ServerState {
    fn get_mode_entry<'a>(
        all_modes: &'a HashMap<String, ModeEntry>,
        mode_name: &str,
    ) -> &'a ModeEntry {
        all_modes.get(mode_name).unwrap_or_else(|| {
            panic!("unknown mode: {}", mode_name);
        })
    }

    /// Load/Execute 用: state(write)
    async fn lock_for_load_or_execute(&self) -> RwLockWriteGuard<'_, State> {
        self.state.write().await
    }

    /// Preview 用: state(read) → mode_name を取得して即 drop
    async fn read_current_mode_name(&self) -> String {
        self.state.read().await.current_mode_name().to_string()
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

    let mut state = server_state.lock_for_load_or_execute().await;
    let mode_name = state.current_mode_name().to_string();
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
    let mode_name = server_state.read_current_mode_name().await;
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

    let mut state = server_state.lock_for_load_or_execute().await;
    let mode_name = state.current_mode_name().to_string();
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
// Dispatch (transform 用: モード切替 + モード依存キーのディスパッチ)

async fn handle_dispatch_request(
    _env: Arc<Env>,
    server_state: ServerState,
    params: method::DispatchParam,
    tx: Arc<Mutex<WriteHalf<UnixStream>>>,
) {
    let method::DispatchParam { key, query, item } = params;

    info!("server: dispatch"; "key" => &key, "query" => &query, "item" => &item);

    let action = dispatch_mode_key(&server_state, &key).await;

    let resp = method::DispatchResp { action };

    let mut tx = tx.lock().await;
    match send_response(method::Dispatch, &mut *tx, &resp).await {
        Ok(()) => trace!("server: dispatch done"),
        Err(e) => error!("server: dispatch error"; "error" => e),
    }
}

/// モード依存キーの dispatch: 現在モードの rendered_bindings から返す
async fn dispatch_mode_key(server_state: &ServerState, key: &str) -> String {
    let mode_name = server_state.read_current_mode_name().await;
    let entry = ServerState::get_mode_entry(&server_state.all_modes, &mode_name);

    match entry.rendered_bindings.get(key) {
        Some(action) => action.clone(),
        None => {
            trace!("server: dispatch no binding"; "mode" => &*mode_name, "key" => key);
            String::new()
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
