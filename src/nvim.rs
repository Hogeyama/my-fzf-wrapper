use std::error::Error;
use std::process::Output;

use ansi_term::ANSIGenericString;
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
    nvim.setup_nvim_config().await?;
    trace!("nvim started");
    Ok(nvim)
}

pub type Neovim = nvim_rs::Neovim<TokioCompat<WriteHalf<Connection>>>;

pub trait NeovimExt {
    async fn setup_nvim_config(&self) -> Result<(), Box<dyn Error>>;

    async fn start_insert(&self) -> Result<(), Box<dyn Error>>;

    async fn stop_insert(&self) -> Result<(), Box<dyn Error>>;

    async fn move_to_last_win(&self) -> Result<(), Box<dyn Error>>;

    async fn move_to_last_tab(&self) -> Result<(), Box<dyn Error>>;

    async fn last_opened_file(&self) -> Result<String, Box<dyn Error>>;

    async fn hide_floaterm(&self) -> Result<(), Box<dyn Error>>;

    async fn open(&self, target: OpenTarget, opts: OpenOpts) -> Result<(), Box<dyn Error>>;

    async fn notify_info(&self, msg: impl AsRef<str>) -> Result<(), Box<dyn Error>>;

    async fn notify_warn(&self, msg: impl AsRef<str>) -> Result<(), Box<dyn Error>>;

    async fn notify_error(&self, msg: impl AsRef<str>) -> Result<(), Box<dyn Error>>;

    async fn notify_command_result(
        &self,
        command: impl AsRef<str>,
        output: Output,
    ) -> Result<(), Box<dyn Error>>;

    async fn notify_command_result_if_error(
        &self,
        command: impl AsRef<str>,
        output: Output,
    ) -> Result<(), Box<dyn Error>>;

    async fn delete_buffer(&self, bufnr: usize, force: bool) -> Result<(), Box<dyn Error>>;

    async fn register_autocommands(&self, autcmds: Vec<(&str, &str)>)
        -> Result<(), Box<dyn Error>>;

    async fn register_command(&self, name: &str, command: &str) -> Result<(), Box<dyn Error>>;

    async fn eval_lua(&self, expr: impl AsRef<str>) -> Result<rmpv::Value, Box<dyn Error>>;

    async fn eval_lua_with_args(
        &self,
        expr: impl AsRef<str>,
        args: Vec<rmpv::Value>,
    ) -> Result<rmpv::Value, Box<dyn Error>>;

    async fn get_all_diagnostics(&self) -> Result<Vec<DiagnosticsItem>, Box<dyn Error>>;

    async fn get_buf_name(&self, bufnr: usize) -> Result<String, Box<dyn Error>>;
}

impl NeovimExt for nvim_rs::Neovim<TokioCompat<WriteHalf<Connection>>> {
    async fn setup_nvim_config(&self) -> Result<(), Box<dyn Error>> {
        // 変数の初期化
        self.exec(
            r#"let g:fzfw_last_win    = 1
           let g:fzfw_last_file   = "."
           let g:fzfw_last_tab    = 1
           let g:fzfw_current_buf = 1"#,
            false,
        )
        .await?;

        // autocommandの初期化
        let _ = self
            .call(
                "nvim_create_augroup",
                call_args!["fzfw", to_value(json!({ "clear": true, }))?],
            )
            .await?
            .map_err(|e| e.to_string())?;

        // autocommandの登録
        self.register_autocommands(vec![
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
        ])
        .await?;

        // commandの登録
        self.register_command(
            "FzfwMoveToLastWin",
            r#"execute "normal! ".g:fzfw_last_win."<C-w><C-w>""#,
        )
        .await?;
        self.register_command("FzfwMoveToLastTab", r#"execute "tabnext ".g:fzfw_last_tab"#)
            .await?;

        Ok(())
    }

    async fn move_to_last_win(self: &Neovim) -> Result<(), Box<dyn Error>> {
        // 何故かコマンドを経由しないと動かなかった
        self.command("FzfwMoveToLastWin").await?;
        Ok(())
    }

    async fn move_to_last_tab(self: &Neovim) -> Result<(), Box<dyn Error>> {
        self.command("FzfwMoveToLastTab").await?;
        Ok(())
    }

    async fn start_insert(&self) -> Result<(), Box<dyn Error>> {
        self.command("startinsert").await?;
        Ok(())
    }

    async fn stop_insert(&self) -> Result<(), Box<dyn Error>> {
        self.command("stopinsert").await?;
        Ok(())
    }

    async fn last_opened_file(&self) -> Result<String, Box<dyn Error>> {
        let r = self.eval("g:fzfw_last_file").await?;
        match r {
            nvim_rs::Value::String(s) => Ok(s.into_str().unwrap()),
            _ => Err("g:fzfw_last_file is not string".into()),
        }
    }

    async fn hide_floaterm(&self) -> Result<(), Box<dyn Error>> {
        self.command("FloatermHide! fzf").await?;
        Ok(())
    }

    async fn open(&self, target: OpenTarget, opts: OpenOpts) -> Result<(), Box<dyn Error>> {
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
                    self.command(&cmd).await.map_err(|e| e.to_string())?;
                    self.move_to_last_tab().await?;
                    Ok(())
                } else {
                    self.stop_insert().await?;
                    self.hide_floaterm().await?;
                    let cmd = format!("execute 'edit {line_opt} '.fnameescape('{file}')");
                    self.command(&cmd).await.map_err(|e| e.to_string())?;
                    Ok(())
                }
            }
            OpenTarget::Buffer(bufnr) => {
                let cmd = format!("buffer {line_opt} {bufnr}");
                if opts.tabedit {
                    self.command("tabnew").await.map_err(|e| e.to_string())?;
                    self.command(&cmd).await.map_err(|e| e.to_string())?;
                    self.move_to_last_tab().await?;
                    Ok(())
                } else {
                    self.stop_insert().await?;
                    self.hide_floaterm().await?;
                    self.command(&cmd).await.map_err(|e| e.to_string())?;
                    Ok(())
                }
            }
        }
    }

    async fn notify_info(&self, msg: impl AsRef<str>) -> Result<(), Box<dyn Error>> {
        self.notify(msg.as_ref(), 2, vec![]).await?;
        Ok(())
    }

    async fn notify_warn(&self, msg: impl AsRef<str>) -> Result<(), Box<dyn Error>> {
        self.notify(msg.as_ref(), 3, vec![]).await?;
        Ok(())
    }

    async fn notify_error(&self, msg: impl AsRef<str>) -> Result<(), Box<dyn Error>> {
        self.notify(msg.as_ref(), 4, vec![]).await?;
        Ok(())
    }

    async fn notify_command_result(
        &self,
        command: impl AsRef<str>,
        output: Output,
    ) -> Result<(), Box<dyn Error>> {
        if output.status.success() {
            self.notify_info(format!(
                "{} succeeded\n{}",
                command.as_ref(),
                String::from_utf8_lossy(output.stdout.as_slice())
            ))
            .await
        } else {
            self.notify_error(format!(
                "{} failed\n{}",
                command.as_ref(),
                String::from_utf8_lossy(output.stderr.as_slice())
            ))
            .await
        }
    }

    async fn notify_command_result_if_error(
        &self,
        command: impl AsRef<str>,
        output: Output,
    ) -> Result<(), Box<dyn Error>> {
        if !output.status.success() {
            self.notify_error(format!(
                "{} failed\n{}",
                command.as_ref(),
                String::from_utf8_lossy(output.stderr.as_slice())
            ))
            .await
        } else {
            Ok(())
        }
    }

    async fn delete_buffer(&self, bufnr: usize, force: bool) -> Result<(), Box<dyn Error>> {
        let cmd = format!("bdelete{} {}", if force { "!" } else { "" }, bufnr);
        info!("delete_buffer: {}", cmd);
        self.exec(&cmd, false).await?;
        Ok(())
    }

    async fn register_autocommands(
        &self,
        autcmds: Vec<(&str, &str)>,
    ) -> Result<(), Box<dyn Error>> {
        let _ = self
            .call(
                "nvim_create_augroup",
                call_args![FZFW_AUTOCMD_GROUP, to_value(json!({ "clear": true, }))?],
            )
            .await?
            .map_err(|e| e.to_string())?;
        for (event, command) in autcmds.iter() {
            let _ = self
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

    async fn register_command(&self, name: &str, command: &str) -> Result<(), Box<dyn Error>> {
        let _ = self
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

    async fn eval_lua(&self, expr: impl AsRef<str>) -> Result<rmpv::Value, Box<dyn Error>> {
        self.eval_lua_with_args(expr, vec![]).await
    }

    async fn eval_lua_with_args(
        &self,
        expr: impl AsRef<str>,
        args: Vec<rmpv::Value>,
    ) -> Result<rmpv::Value, Box<dyn Error>> {
        let v = self
            .call("nvim_exec_lua", call_args![expr.as_ref(), args])
            .await?
            .map_err(|e| e.to_string())?;
        Ok(v)
    }

    async fn get_all_diagnostics(&self) -> Result<Vec<DiagnosticsItem>, Box<dyn Error>> {
        Ok(from_value(
            self.eval_lua(
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

    async fn get_buf_name(&self, bufnr: usize) -> Result<String, Box<dyn Error>> {
        Ok(from_value(
            self.eval_lua(&format!("return vim.api.nvim_buf_get_name({bufnr})"))
                .await?,
        )?)
    }
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

const FZFW_AUTOCMD_GROUP: &str = "fzfw";
