use std::error::Error;

use crate::{
    external_command::fd,
    method::{Load, LoadResp, Method, PreviewResp, RunResp},
    nvim::{hide_floaterm, move_to_last_tab, stop_insert, Neovim},
    types::{Mode, State},
};

use clap::Parser;
use futures::{future::BoxFuture, FutureExt};
use tokio::process::Command as TokioCommand;

use super::utils as mode_utils;
use crate::utils;

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
        opts: Vec<String>,
    ) -> BoxFuture<'a, <Load as Method>::Response> {
        let nvim = state.nvim.clone();
        async move {
            match utils::clap_parse_from::<LoadOpts>(opts) {
                Ok(opts) => {
                    if let Err(e) = mode_utils::change_directory(&nvim, opts.into()).await {
                        error!("fd: load: change_directory failed"; "error" => e);
                    }
                }
                Err(e) => {
                    error!("fd.run.opts failed"; "error" => e.to_string());
                }
            }
            let fd_result = fd::new().output().await;
            match fd_result {
                Ok(fd_output) => {
                    let pwd = std::env::current_dir().unwrap().into_os_string();
                    let fd_output = String::from_utf8_lossy(&fd_output.stdout)
                        .lines()
                        .map(|line| line.to_string())
                        .collect::<Vec<_>>();
                    LoadResp {
                        header: format!("[{}]", pwd.to_string_lossy()),
                        items: fd_output,
                    }
                }
                Err(fd_err) => {
                    error!("fd.run.opts failed"; "error" => fd_err.to_string());
                    LoadResp {
                        header: fd_err.to_string(),
                        items: vec![],
                    }
                }
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

#[derive(Parser, Debug, Clone)]
struct LoadOpts {
    // Change directory to the given dir.
    // If a file is specified, change to the directory containing the file.
    #[clap(long)]
    cd: Option<String>,

    // Change directory to the parent directory.
    #[clap(long)]
    cd_up: bool,

    // Change directory to the parent directory.
    #[clap(long)]
    cd_last_file: bool,
}

impl From<LoadOpts> for mode_utils::CdOpts {
    fn from(val: LoadOpts) -> mode_utils::CdOpts {
        mode_utils::CdOpts {
            cd: val.cd,
            cd_up: val.cd_up,
            cd_last_file: val.cd_last_file,
        }
    }
}

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
        stop_insert(&nvim).await?;
        hide_floaterm(&nvim).await?;
        let cmd = format!("execute 'edit {line_opt} '.fnameescape('{path}')");
        nvim.command(&cmd).await.map_err(|e| e.to_string())?;
        Ok(())
    }
}
