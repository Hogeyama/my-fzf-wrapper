use std::error::Error;

use crate::{
    external_command::fd,
    method::{Load, LoadResp, Method, PreviewResp, RunResp},
    nvim::{focusing_last_win, leaving_insert_mode, move_to_last_tab, Neovim},
    types::{Mode, State},
};

use clap::Parser;
use futures::{future::BoxFuture, FutureExt};
use tokio::process::Command as TokioCommand;

#[derive(Clone)]
pub struct Fd;

pub fn new() -> Fd {
    Fd
}

impl Mode for Fd {
    fn name(&self) -> &'static str {
        "fd"
    }
    fn load<'a>(
        &self,
        state: &'a mut State,
        _arg: Vec<String>,
    ) -> BoxFuture<'a, <Load as Method>::Response> {
        async {
            let fd_result = fd::new().output().await;
            match fd_result {
                Ok(fd_output) => {
                    let pwd = state.pwd.clone().into_os_string();
                    let fd_output = String::from_utf8_lossy(&fd_output.stdout)
                        .lines()
                        .map(|line| line.to_string())
                        .collect::<Vec<_>>();
                    LoadResp {
                        header: format!("[{}]", pwd.to_string_lossy()),
                        items: fd_output,
                    }
                }
                Err(fd_err) => LoadResp {
                    header: fd_err.to_string(),
                    items: vec![],
                },
            }
        }
        .boxed()
    }
    fn preview(&self, _state: &mut State, item: String) -> BoxFuture<'static, PreviewResp> {
        async move {
            let output = TokioCommand::new("bat")
                .arg(&item)
                .args(vec!["--color", "always"])
                .output()
                .await
                .map_err(|e| e.to_string())
                .expect("fd: preview:")
                .stdout;
            let output = String::from_utf8_lossy(output.as_slice()).into_owned();
            PreviewResp { message: output }
        }
        .boxed()
    }
    fn run<'a>(
        &self,
        state: &'a mut State,
        path: String,
        opts: Vec<String>,
    ) -> BoxFuture<'a, RunResp> {
        async move {
            let nvim = state.nvim.clone();
            match clap_parse_from::<RunOpts>(opts) {
                Ok(opts) => {
                    let _ = tokio::spawn(async move {
                        let r = nvim_open(&nvim, path.clone(), opts).await;
                        if let Err(e) = r {
                            error!("fd: run: nvim_open failed"; "error" => e.to_string());
                        }
                    });
                }
                Err(e) => {
                    error!("fd.run.opts failed"; "error" => e.to_string());
                }
            }
            RunResp
        }
        .boxed()
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Load
////////////////////////////////////////////////////////////////////////////////////////////////////

////////////////////////////////////////////////////////////////////////////////////////////////////
// Run
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Parser, Debug, Clone)]
struct RunOpts {
    #[clap(long)]
    line: Option<u32>,

    #[clap(long)]
    tabedit: bool,
}

async fn nvim_open(nvim: &Neovim, path: String, opts: RunOpts) -> Result<(), Box<dyn Error>> {
    let line_opt = match opts.line {
        Some(line) => format!("+{line}"),
        None => "".to_string(),
    };
    let path = std::fs::canonicalize(path).unwrap();
    let path = path.to_string_lossy();

    if opts.tabedit {
        let cmd = format!("execute 'tabedit {line_opt} '.fnameescape('{path}')",);
        // open in new tab
        nvim.command(&cmd).await.map_err(|e| e.to_string())?;
        // return to fzf tab
        move_to_last_tab(&nvim).await?;
        Ok(())
    } else {
        leaving_insert_mode(&nvim, || {
            async {
                focusing_last_win(&nvim, || {
                    async {
                        let cmd = format!("execute 'edit {line_opt} '.fnameescape('{path}')");
                        nvim.command(&cmd).await.map_err(|e| e.to_string())?;
                        Ok(())
                    }
                    .boxed()
                })
                .await
            }
            .boxed()
        })
        .await
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Util
////////////////////////////////////////////////////////////////////////////////////////////////////

fn clap_parse_from<T: Parser>(args: Vec<String>) -> Result<T, clap::error::Error> {
    let mut clap_args = vec!["dummy".to_string()];
    clap_args.extend(args);
    T::try_parse_from(clap_args)
}
