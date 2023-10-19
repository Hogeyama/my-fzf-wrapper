use crate::{
    external_command::{fd, fzf},
    method::{Load, LoadResp, Method, PreviewResp, RunOpts, RunResp},
    nvim,
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
    fn load(
        &mut self,
        state: &mut State,
        opts: Vec<String>,
    ) -> BoxFuture<'static, <Load as Method>::Response> {
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
                    let fd_output = String::from_utf8_lossy(&fd_output.stdout)
                        .lines()
                        .map(|line| line.to_string())
                        .collect::<Vec<_>>();
                    LoadResp::new_with_default_header(fd_output)
                }
                Err(fd_err) => {
                    error!("fd.run.opts failed"; "error" => fd_err.to_string());
                    LoadResp::new(fd_err.to_string(), vec![])
                }
            }
        }
        .boxed()
    }
    fn preview(&mut self, _state: &mut State, item: String) -> BoxFuture<'static, PreviewResp> {
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
    fn run(
        &mut self,
        state: &mut State,
        path: String,
        opts: RunOpts,
    ) -> BoxFuture<'static, RunResp> {
        let nvim = state.nvim.clone();
        async move {
            let vifm = || async {
                let pwd = std::env::current_dir().unwrap().into_os_string();
                TokioCommand::new("vifm")
                    .arg(&pwd)
                    .spawn()
                    .unwrap()
                    .wait()
                    .await
                    .unwrap();
            };
            let browse_github = || async {
                TokioCommand::new("gh")
                    .arg("browse")
                    .arg(&path)
                    .spawn()
                    .unwrap()
                    .wait()
                    .await
                    .unwrap();
            };
            let path_ = path.clone();
            let nvim_opt = opts.clone().into();
            let nvim = || async {
                let _ = tokio::spawn(async move {
                    let r = nvim::open(&nvim, path_.into(), nvim_opt).await;
                    if let Err(e) = r {
                        error!("fd: run: nvim::open failed"; "error" => e.to_string());
                    }
                });
            };
            match () {
                _ if opts.menu => {
                    let items = vec!["browse-github", "vifm"];
                    match &*fzf::select(items).await {
                        "vifm" => vifm().await,
                        "browse-github" => browse_github().await,
                        _ => {}
                    }
                }
                _ if opts.vifm => vifm().await,
                _ if opts.browse_github => browse_github().await,
                _ => nvim().await,
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
