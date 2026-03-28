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
    let fzf_client = Arc::new(fzf::FzfClient::new(&fzf_listen_socket));

    // 統合バインディングをレンダリング
    let rendered_bindings = config.render_mode_bindings(&unified_bindings);

    // 統合 fzf 設定で起動 (--no-sort + --multi を付与)
    let fzf_config = fzf::Config {
        default_command: shellwords::join(&[config.myself.as_ref(), "load", "default", "", ""]),
        preview_command: format!("{} preview {{}}", config.myself),
        socket: config.socket.clone(),
        log_file: config.log_file.clone(),
        initial_prompt,
        initial_query: "".to_string(),
        rendered_bindings,
        extra_opts: vec!["--no-sort".to_string(), "--multi".to_string()],
        listen_socket: Some(fzf_listen_socket),
    };

    // Env を構築 (LoadState / ModeState を含む)
    let env = Arc::new(Env {
        config,
        nvim,
        fzf_client: fzf_client.clone(),
        mode_infos: Arc::new(mode_infos),
        last_load_resp: Mutex::new(None),
        mode: Arc::new(RwLock::new(crate::env::ModeState::new(
            initial_mode_name,
            initial_sort,
        ))),
    });

    let server_state = ServerState {
        fzf: Arc::new(RwLock::new(
            fzf::new(fzf_config)
                .stdout(std::process::Stdio::piped())
                .spawn()
                .expect("Failed to spawn fzf"),
        )),
        all_modes,
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

/// Env 内の load / mode は独立ロック。load 中でもモード名の読み書きはブロックされない。
#[derive(Clone)]
struct ServerState {
    fzf: Arc<RwLock<Child>>,
    all_modes: Arc<HashMap<String, ModeEntry>>,
}

impl ServerState {
    fn get_mode_entry<'a>(
        all_modes: &'a HashMap<String, ModeEntry>,
        mode_name: &str,
    ) -> &'a ModeEntry {
        all_modes.get(mode_name).unwrap_or_else(|| {
            panic!("unknown mode: {}", mode_name);
        })
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

    let mode_name = env.mode.read().await.current_mode_name().to_string();
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

    let last_resp = {
        let stream = callback(
            entry.mode.mode_def.as_ref(),
            &env,
            query,
            item.unwrap_or_default(),
        );
        send_load_stream(stream, tx).await
    };
    *env.last_load_resp.lock().await = last_resp;
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
    let mode_name = env.mode.read().await.current_mode_name().to_string();
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
        cursor_pos,
    } = params;

    // _key: プレフィックス付きの場合: キー dispatch (rendered_bindings を POST)
    if let Some(key) = registered_name.strip_prefix("_key:") {
        let mode_name = env.mode.read().await.current_mode_name().to_string();
        let entry = ServerState::get_mode_entry(&server_state.all_modes, &mode_name);

        let cursor_pos_str = cursor_pos.as_deref().unwrap_or("");
        let action = match entry.rendered_bindings.get(key) {
            Some(action) => action
                .replace("{q}", &shellwords::escape(&query))
                .replace("{}", &shellwords::escape(&item))
                .replace("{n}", &shellwords::escape(cursor_pos_str)),
            None => {
                trace!("server: execute _key: no binding"; "mode" => &*mode_name, "key" => key);
                String::new()
            }
        };

        if !action.is_empty() {
            if let Err(e) = env.fzf_client.post_action(&action).await {
                error!("server: execute _key: post_action error"; "error" => e.to_string());
            }
        }
    } else {
        // 通常のコールバック実行
        let mode_name = env.mode.read().await.current_mode_name().to_string();
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

        match callback(entry.mode.mode_def.as_ref(), &env, query, item).await {
            Ok(_) => {}
            Err(e) => error!("server: execute error"; "error" => e.to_string()),
        }
    }

    let mut tx = tx.lock().await;
    match send_response(method::Execute, &mut *tx, &()).await {
        Ok(()) => info!("server: execute done"),
        Err(e) => error!("server: execute error"; "error" => e),
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
