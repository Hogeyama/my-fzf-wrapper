use crate::{
    external_command::{bat, fd, fzf},
    method::{LoadResp, PreviewResp, RunOpts, RunResp},
    nvim,
    types::{Mode, State},
};

use clap::Parser;
use futures::{future::BoxFuture, FutureExt};
use tokio::process::Command;

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
    ) -> BoxFuture<'static, Result<LoadResp, String>> {
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
            let fd_output = fd::new().output().await.map_err(|e| e.to_string())?;
            let fd_output = String::from_utf8_lossy(&fd_output.stdout)
                .lines()
                .map(|line| line.to_string())
                .collect::<Vec<_>>();
            Ok(LoadResp::new_with_default_header(fd_output))
        }
        .boxed()
    }
    fn preview(
        &mut self,
        _state: &mut State,
        item: String,
    ) -> BoxFuture<'static, Result<PreviewResp, String>> {
        async move {
            let message = bat::render_file(&item).await?;
            Ok(PreviewResp { message })
        }
        .boxed()
    }
    fn run(
        &mut self,
        state: &mut State,
        path: String,
        opts: RunOpts,
    ) -> BoxFuture<'static, Result<RunResp, String>> {
        let nvim = state.nvim.clone();
        async move {
            let vifm = || async {
                let pwd = std::env::current_dir().unwrap().into_os_string();
                Command::new("vifm")
                    .arg(&pwd)
                    .spawn()
                    .map_err(|e| e.to_string())?
                    .wait()
                    .await
                    .map_err(|e| e.to_string())?;
                Ok::<(), String>(())
            };
            let browse_github = || async {
                Command::new("gh")
                    .arg("browse")
                    .arg(&path)
                    .spawn()
                    .map_err(|e| e.to_string())?
                    .wait()
                    .await
                    .map_err(|e| e.to_string())?;
                Ok::<(), String>(())
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
                    match &*fzf::select(items).await? {
                        "vifm" => vifm().await?,
                        "browse-github" => browse_github().await?,
                        _ => (),
                    }
                }
                _ if opts.vifm => vifm().await?,
                _ if opts.browse_github => browse_github().await?,
                _ => nvim().await,
            }
            Ok(RunResp)
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
