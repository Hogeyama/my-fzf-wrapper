use crate::{
    external_command::{bat, fd, fzf, gh},
    method::{LoadResp, PreviewResp, RunOpts, RunResp},
    nvim,
    types::{Mode, State},
};

use futures::{future::BoxFuture, FutureExt};
use tokio::process::Command;

#[derive(Clone)]
pub struct Fd;

impl Mode for Fd {
    fn new() -> Self {
        Fd
    }
    fn name(&self) -> &'static str {
        "fd"
    }
    fn load(
        &self,
        _state: &mut State,
        _opts: Vec<String>,
    ) -> BoxFuture<'static, Result<LoadResp, String>> {
        async move {
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
        &self,
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
        &self,
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
                gh::browse_github(&path).await?;
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
