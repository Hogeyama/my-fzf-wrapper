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
use crate::types::State;
use crate::Config;

pub async fn server<'a>(
    config: &'a Config,
    mut state: State<'a>,
    listener: UnixListener,
) -> Result<(), String> {
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
                    let method::LoadParam { mode, args } = params;
                    state.mode = config.get_mode(mode);
                    let resp = state.mode.load(&mut state, args).await;
                    send_response(&mut tx, method, resp).await?;
                }
                Some(method::Request::Preview { method, params }) => {
                    let method::PreviewParam { item } = params;
                    let resp = state.mode.preview(&mut state, item).await;
                    send_response(&mut tx, method, resp).await?;
                }
                Some(method::Request::Run { method, params }) => {
                    let method::RunParam { item, args } = params;
                    let resp = state.mode.run(&mut state, item, args).await;
                    send_response(&mut tx, method, resp).await?;
                }
                Some(method::Request::Reload { method, params: () }) => {
                    let args = state.last_load.args.clone();
                    let resp = state.mode.load(&mut state, args).await;
                    send_response(&mut tx, method, resp).await?;
                }
                None => {
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
    let resp = serde_json::to_string(&resp).expect("nyaan") + "\n";
    tx.write_all(resp.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
