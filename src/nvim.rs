use std::error::Error;
use std::process::Output;

use ansi_term::ANSIGenericString;
use futures::future::BoxFuture;
// Neovim
use nvim_rs::compat::tokio::Compat as TokioCompat;
use nvim_rs::create::tokio as nvim_tokio;
use nvim_rs::rpc::model::IntoVal;
use nvim_rs::{call_args, Handler};

// Tokio
use parity_tokio_ipc::Connection;
use rmpv::ext::{from_value, to_value};
use serde::{Deserialize, Serialize};
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
    setup_nvim_config(&nvim).await?;
    trace!("nvim started");
    Ok(nvim)
}

pub type Neovim = nvim_rs::Neovim<TokioCompat<WriteHalf<Connection>>>;

////////////////////////////////////////////////////////////////////////////////
// Utils
////////////////////////////////////////////////////////////////////////////////

#[allow(dead_code)]
pub async fn move_to_last_win(nvim: &Neovim) -> Result<(), Box<dyn Error>> {
    // 何故かコマンドを経由しないと動かなかった
    nvim.command("FzfwMoveToLastWin").await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn move_to_last_tab(nvim: &Neovim) -> Result<(), Box<dyn Error>> {
    nvim.command("FzfwMoveToLastTab").await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn start_insert(nvim: &Neovim) -> Result<(), Box<dyn Error>> {
    nvim.command("startinsert").await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn stop_insert(nvim: &Neovim) -> Result<(), Box<dyn Error>> {
    nvim.command("stopinsert").await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn leaving_insert_mode<T>(
    nvim: &Neovim,
    callback: impl Fn() -> BoxFuture<'static, Result<T, Box<dyn Error>>>,
) -> Result<T, Box<dyn Error>> {
    stop_insert(nvim).await?;
    let r = callback().await?;
    start_insert(nvim).await?;
    Ok(r)
}

#[allow(dead_code)]
pub async fn focusing_last_win<T>(
    nvim: &Neovim,
    callback: impl Fn() -> BoxFuture<'static, Result<T, Box<dyn Error>>>,
) -> Result<T, Box<dyn Error>> {
    move_to_last_win(nvim).await?;
    let r = callback().await?;
    move_to_last_win(nvim).await?;
    Ok(r)
}

#[allow(dead_code)]
pub async fn focusing_last_tab<T>(
    nvim: &Neovim,
    callback: impl Fn() -> BoxFuture<'static, Result<T, Box<dyn Error>>>,
) -> Result<T, Box<dyn Error>> {
    move_to_last_tab(nvim).await?;
    let r = callback().await?;
    move_to_last_tab(nvim).await?;
    Ok(r)
}

#[allow(dead_code)]
pub async fn last_opened_file(nvim: &Neovim) -> Result<String, Box<dyn Error>> {
    let r = nvim.eval("g:fzfw_last_file").await?;
    match r {
        nvim_rs::Value::String(s) => Ok(s.into_str().unwrap()),
        _ => Err("g:fzfw_last_file is not string".into()),
    }
}

#[allow(dead_code)]
pub async fn hide_floaterm(nvim: &Neovim) -> Result<(), Box<dyn Error>> {
    nvim.command("FloatermHide! fzf").await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn open(nvim: &Neovim, target: OpenTarget, opts: OpenOpts) -> Result<(), Box<dyn Error>> {
    let line_opt = match opts.line {
        Some(line) => format!("+{line}"),
        None => "".to_string(),
    };
    match target {
        OpenTarget::File(file) => {
            let file = std::fs::canonicalize(file).map_err(|e| e.to_string())?;
            let file = file.to_string_lossy();
            if opts.tabedit {
                let cmd = format!("execute 'tabedit {line_opt} '.fnameescape('{file}')",);
                nvim.command(&cmd).await.map_err(|e| e.to_string())?;
                move_to_last_tab(nvim).await?;
                Ok(())
            } else {
                stop_insert(nvim).await?;
                hide_floaterm(nvim).await?;
                let cmd = format!("execute 'edit {line_opt} '.fnameescape('{file}')");
                nvim.command(&cmd).await.map_err(|e| e.to_string())?;
                Ok(())
            }
        }
        OpenTarget::Buffer(bufnr) => {
            let cmd = format!("buffer {line_opt} {bufnr}");
            if opts.tabedit {
                nvim.command("tabnew").await.map_err(|e| e.to_string())?;
                nvim.command(&cmd).await.map_err(|e| e.to_string())?;
                move_to_last_tab(nvim).await?;
                Ok(())
            } else {
                stop_insert(nvim).await?;
                hide_floaterm(nvim).await?;
                nvim.command(&cmd).await.map_err(|e| e.to_string())?;
                Ok(())
            }
        }
    }
}

async fn notify(nvim: &Neovim, msg: impl AsRef<str>, level: i64) -> Result<(), Box<dyn Error>> {
    eval_lua_with_args(nvim, r#"vim.notify(...)"#, call_args![msg.as_ref(), level]).await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn notify_info(nvim: &Neovim, msg: impl AsRef<str>) -> Result<(), Box<dyn Error>> {
    notify(nvim, msg, 2).await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn notify_warn(nvim: &Neovim, msg: impl AsRef<str>) -> Result<(), Box<dyn Error>> {
    notify(nvim, msg, 3).await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn notify_error(nvim: &Neovim, msg: impl AsRef<str>) -> Result<(), Box<dyn Error>> {
    notify(nvim, msg, 4).await?;
    Ok(())
}

pub async fn notify_command_result(
    nvim: &Neovim,
    command: impl AsRef<str>,
    output: Output,
) -> Result<(), Box<dyn Error>> {
    if output.status.success() {
        notify_info(
            nvim,
            format!(
                "{} succeeded\n{}",
                command.as_ref(),
                String::from_utf8_lossy(output.stdout.as_slice())
            ),
        )
        .await
    } else {
        notify_error(
            nvim,
            format!(
                "{} failed\n{}",
                command.as_ref(),
                String::from_utf8_lossy(output.stderr.as_slice())
            ),
        )
        .await
    }
}

#[allow(dead_code)]
pub async fn delete_buffer(nvim: &Neovim, bufnr: usize, force: bool) -> Result<(), Box<dyn Error>> {
    let cmd = format!("bdelete{} {}", if force { "!" } else { "" }, bufnr);
    info!("delete_buffer: {}", cmd);
    nvim.exec(&cmd, false).await?;
    Ok(())
}

pub struct OpenOpts {
    pub line: Option<usize>,
    pub tabedit: bool,
}

pub enum OpenTarget {
    File(String),
    Buffer(usize),
}

impl From<String> for OpenTarget {
    fn from(val: String) -> Self {
        OpenTarget::File(val)
    }
}

impl From<usize> for OpenTarget {
    fn from(val: usize) -> Self {
        OpenTarget::Buffer(val)
    }
}

#[allow(dead_code)]
pub async fn get_all_diagnostics(nvim: &Neovim) -> Result<Vec<DiagnosticsItem>, Box<dyn Error>> {
    Ok(from_value(
        eval_lua(
            nvim,
            r#"
            local ds = vim.diagnostic.get()
            for _, d in ipairs(ds) do
              d.file = vim.api.nvim_buf_get_name(d.bufnr)
            end
            return ds
            "#,
        )
        .await?,
    )?)
}

#[allow(dead_code)]
pub async fn get_buf_name(nvim: &Neovim, bufnr: usize) -> Result<String, Box<dyn Error>> {
    Ok(from_value(
        eval_lua(nvim, &format!("return vim.api.nvim_buf_get_name({bufnr})")).await?,
    )?)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticsItem {
    pub bufnr: u64,
    pub file: String,
    pub lnum: u64,
    pub col: u64,
    pub message: String,
    pub severity: Severity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Severity(pub u64);

impl Severity {
    pub fn mark(&self) -> ANSIGenericString<'_, str> {
        match self.0 {
            1 => ansi_term::Colour::Red.bold().paint("E"),
            2 => ansi_term::Colour::Yellow.bold().paint("W"),
            3 => ansi_term::Colour::Blue.bold().paint("I"),
            4 => ansi_term::Colour::White.normal().paint("H"),
            _ => panic!("unknown severity {}", self.0),
        }
    }
    pub fn render(&self) -> String {
        match self.0 {
            1 => "Error".to_string(),
            2 => "Warning".to_string(),
            3 => "Info".to_string(),
            4 => "Hint".to_string(),
            _ => panic!("unknown severity {}", self.0),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Impl
////////////////////////////////////////////////////////////////////////////////

async fn setup_nvim_config(nvim: &Neovim) -> Result<(), Box<dyn Error>> {
    // 変数の初期化
    nvim.exec(
        r#"let g:fzfw_last_win    = 1
           let g:fzfw_last_file   = "."
           let g:fzfw_last_tab    = 1
           let g:fzfw_current_buf = 1"#,
        false,
    )
    .await?;

    // autocommandの初期化
    let _ = nvim
        .call(
            "nvim_create_augroup",
            call_args!["fzfw", to_value(json!({ "clear": true, }))?],
        )
        .await?
        .map_err(|e| e.to_string())?;

    // autocommandの登録
    register_autocmds(
        nvim,
        vec![
            ("WinLeave", r#"let g:fzfw_last_win = winnr()"#),
            ("WinLeave", r#"let g:fzfw_last_file = expand("%:p")"#),
            ("TabLeave", r#"let g:fzfw_last_tab = tabpagenr()"#),
            (
                "BufLeave",
                &[
                    r#"let g:fzfw_last_buf = g:fzfw_current_buf"#,
                    r#"let g:fzfw_current_buf = bufnr('%')"#,
                ]
                .join("|"),
            ),
        ],
    )
    .await?;

    // commandの登録
    register_command(
        nvim,
        "FzfwMoveToLastWin",
        r#"execute "normal! ".g:fzfw_last_win."<C-w><C-w>""#,
    )
    .await?;
    register_command(
        nvim,
        "FzfwMoveToLastTab",
        r#"execute "tabnext ".g:fzfw_last_tab"#,
    )
    .await?;

    Ok(())
}

async fn register_autocmds(
    nvim: &Neovim,
    autcmds: Vec<(&str, &str)>,
) -> Result<(), Box<dyn Error>> {
    let _ = nvim
        .call(
            "nvim_create_augroup",
            call_args![FZFW_AUTOCMD_GROUP, to_value(json!({ "clear": true, }))?],
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
                        "group": FZFW_AUTOCMD_GROUP,
                        "command": command
                    }))?
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
                }))?
            ],
        )
        .await?
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn eval_lua(nvim: &Neovim, expr: impl AsRef<str>) -> Result<rmpv::Value, Box<dyn Error>> {
    eval_lua_with_args(nvim, expr, vec![]).await
}

pub async fn eval_lua_with_args(
    nvim: &Neovim,
    expr: impl AsRef<str>,
    args: Vec<rmpv::Value>,
) -> Result<rmpv::Value, Box<dyn Error>> {
    let v = nvim
        .call("nvim_exec_lua", call_args![expr.as_ref(), args])
        .await?
        .map_err(|e| e.to_string())?;
    Ok(v)
}

const FZFW_AUTOCMD_GROUP: &str = "fzfw";
