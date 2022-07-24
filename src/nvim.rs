use std::error::Error;

use futures::future::BoxFuture;
// Neovim
use nvim_rs::compat::tokio::Compat as TokioCompat;
use nvim_rs::create::tokio as nvim_tokio;
use nvim_rs::rpc::model::IntoVal;
use nvim_rs::{call_args, Handler};

// Tokio
use parity_tokio_ipc::Connection;
use rmpv::ext::to_value;
use tokio::io::WriteHalf;

#[derive(Clone)]
struct NeovimHandler {}

pub fn _to_nvim_error(err: impl ToString) -> rmpv::Value {
    rmpv::Value::String(rmpv::Utf8String::from(err.to_string()))
}

impl Handler for NeovimHandler {
    type Writer = TokioCompat<WriteHalf<Connection>>;
}

pub async fn start_nvim(nvim_listen_address: &str) -> Result<Neovim, Box<dyn Error>> {
    let handler: NeovimHandler = NeovimHandler {};
    let (nvim, _io_handler) = nvim_tokio::new_path(nvim_listen_address, handler)
        .await
        .expect("Connect to nvim failed");

    let _ = nvim
        .call(
            "nvim_create_augroup",
            call_args![
                "my-fzf-wrapper",
                to_value(json!({ "clear": true, })).unwrap()
            ],
        )
        .await?
        .map_err(|e| e.to_string())?;

    register_autocmds(
        &nvim,
        vec![
            ("WinLeave", r#"let g:myfzf_last_win = winnr()"#),
            ("WinLeave", r#"let g:myfzf_last_file = expand("%:p")"#),
            ("TabLeave", r#"let g:myfzf_last_tab = tabpagenr()"#),
            (
                "BufLeave",
                &vec![
                    r#"let g:myfzf_last_buf = get(g:, 'myfzf_current_buf', 0)"#,
                    r#"let g:myfzf_current_buf = bufnr('%')"#,
                ]
                .join("|"),
            ),
        ],
    )
    .await?;

    register_command(
        &nvim,
        "MyFzfMoveToLastWin",
        r#"execute "normal! ".g:myfzf_last_win."<C-w><C-w>""#,
    )
    .await?;

    register_command(
        &nvim,
        "MyFzfMoveToLastTab",
        r#"execute "tabnext ".g:myfzf_last_tab"#,
    )
    .await?;

    info!("nvim started");
    Ok(nvim)
}

pub type Neovim = nvim_rs::Neovim<TokioCompat<WriteHalf<Connection>>>;

////////////////////////////////////////////////////////////////////////////////
// Utils
////////////////////////////////////////////////////////////////////////////////

#[allow(dead_code)]
pub async fn move_to_last_win(nvim: &Neovim) -> Result<(), Box<dyn Error>> {
    // 何故かコマンドを経由しないと動かなかった
    let _ = nvim.command("MyFzfMoveToLastWin").await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn move_to_last_tab(nvim: &Neovim) -> Result<(), Box<dyn Error>> {
    let _ = nvim.command("MyFzfMoveToLastTab").await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn start_insert(nvim: &Neovim) -> Result<(), Box<dyn Error>> {
    let _ = nvim.command("startinsert").await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn stop_insert(nvim: &Neovim) -> Result<(), Box<dyn Error>> {
    let _ = nvim.command("stopinsert").await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn leaving_insert_mode<'a, T>(
    nvim: &Neovim,
    callback: impl Fn() -> BoxFuture<'a, Result<T, Box<dyn Error>>>,
) -> Result<T, Box<dyn Error>> {
    stop_insert(&nvim).await?;
    let r = callback().await?;
    start_insert(&nvim).await?;
    Ok(r)
}

#[allow(dead_code)]
pub async fn focusing_last_win<'a, T>(
    nvim: &Neovim,
    callback: impl Fn() -> BoxFuture<'a, Result<T, Box<dyn Error>>>,
) -> Result<T, Box<dyn Error>> {
    move_to_last_win(nvim).await?;
    let r = callback().await?;
    move_to_last_win(nvim).await?;
    Ok(r)
}

#[allow(dead_code)]
pub async fn focusing_last_tab<'a, T>(
    nvim: &Neovim,
    callback: impl Fn() -> BoxFuture<'a, Result<T, Box<dyn Error>>>,
) -> Result<T, Box<dyn Error>> {
    move_to_last_tab(nvim).await?;
    let r = callback().await?;
    move_to_last_tab(nvim).await?;
    Ok(r)
}

#[allow(dead_code)]
pub async fn last_opened_file(nvim: &Neovim) -> Result<String, Box<dyn Error>> {
    let r = nvim.eval("g:myfzf_last_file").await?;
    match r {
        nvim_rs::Value::String(s) => Ok(s.into_str().unwrap()),
        _ => Err("g:myfzf_last_file is not string".into()),
    }
}

////////////////////////////////////////////////////////////////////////////////
// Impl
////////////////////////////////////////////////////////////////////////////////

async fn register_autocmds(
    nvim: &Neovim,
    autcmds: Vec<(&str, &str)>,
) -> Result<(), Box<dyn Error>> {
    let _ = nvim
        .call(
            "nvim_create_augroup",
            call_args![
                MYFZF_AUTOCMD_GROUP,
                to_value(json!({ "clear": true, })).unwrap()
            ],
        )
        .await?
        .map_err(|e| e.to_string())?;
    for (event, command) in autcmds.iter() {
        let _ = nvim
            .call(
                "nvim_create_autocmd",
                call_args![
                    event,
                    to_value(json!({
                        "group": MYFZF_AUTOCMD_GROUP,
                        "command": command
                    }))
                    .unwrap()
                ],
            )
            .await?
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

async fn register_command(nvim: &Neovim, name: &str, command: &str) -> Result<(), Box<dyn Error>> {
    let _ = nvim
        .call(
            "nvim_create_user_command",
            call_args![
                name,
                command,
                to_value(json!({
                    "force": true,
                }))
                .unwrap()
            ],
        )
        .await?
        .map_err(|e| e.to_string())?;
    Ok(())
}

const MYFZF_AUTOCMD_GROUP: &str = "my-fzf-wrapper";
