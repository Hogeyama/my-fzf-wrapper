use crate::{
    config::Config,
    method::{LoadResp, PreviewResp},
    mode::{config_builder, ModeDef},
    nvim::{self, NeovimExt},
    state::State,
    utils::{bat, fd, fzf, gh, xsel},
};

use futures::{future::BoxFuture, FutureExt};
use tokio::process::Command;

use super::CallbackMap;

#[derive(Clone)]
pub struct Fd;

impl ModeDef for Fd {
    fn name(&self) -> &'static str {
        "fd"
    }
    fn load(
        &mut self,
        _config: &Config,
        _state: &mut State,
        _query: String,
        _item: String,
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
        _config: &Config,
        _state: &mut State,
        item: String,
    ) -> BoxFuture<'static, Result<PreviewResp, String>> {
        async move {
            let message = bat::render_file(&item).await?;
            Ok(PreviewResp { message })
        }
        .boxed()
    }
    fn fzf_bindings(&self) -> (fzf::Bindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [
                execute!(b, |_mode,_config,state,_query,item| {
                    let opts = OpenOpts::Neovim { tabedit: false };
                    open(state, item, opts).await
                })
            ],
            "ctrl-t" => [
                execute!(b, |_mode,_config,state,_query,item| {
                    let opts = OpenOpts::Neovim { tabedit: true };
                    open(state, item, opts).await
                })
            ],
            "ctrl-v" => [
                execute!(b, |_mode,_config,state,_query,item| {
                    let opts = OpenOpts::Vifm;
                    open(state, item, opts).await
                })
            ],
            "ctrl-y" => [
                execute!(b, |_mode,_config,_state,_query,item| {
                    xsel::yank(item).await?;
                    Ok(())
                })
            ],
            "ctrl-space" => [
                select_and_execute!{b, |_mode,_config,state,_query,item|
                    "neovim" => {
                        let opts = OpenOpts::Neovim { tabedit: false };
                        open(state, item, opts).await
                    },
                    "vifm" => {
                        let opts = OpenOpts::Vifm;
                        open(state, item, opts).await
                    },
                    "browse-github" => {
                        let opts = OpenOpts::BrowseGithub;
                        open(state, item, opts).await
                    },
                    "new file" => {
                        let cwd = std::env::current_dir().unwrap();
                        let fname = fzf::input("Enter file name").await?;
                        let fname = fname.trim();
                        let path = format!("{}/{}", cwd.display(), fname);
                        Command::new("touch")
                            .arg(&path)
                            .output()
                            .await
                            .map_err(|e| e.to_string())?;
                        let opts = OpenOpts::Neovim { tabedit: false };
                        open(state, path, opts).await
                    },
                }
            ]
        }
    }
}

enum OpenOpts {
    Neovim { tabedit: bool },
    Vifm,
    BrowseGithub,
}

async fn open(state: &mut State, file: String, opts: OpenOpts) -> Result<(), String> {
    match opts {
        OpenOpts::Neovim { tabedit } => {
            let nvim = state.nvim.clone();
            let nvim_opts = nvim::OpenOpts {
                line: None,
                tabedit,
            };
            nvim.open(file.into(), nvim_opts)
                .await
                .map_err(|e| e.to_string())?;
        }
        OpenOpts::Vifm => {
            let pwd = std::env::current_dir().unwrap().into_os_string();
            Command::new("vifm")
                .arg(&pwd)
                .spawn()
                .map_err(|e| e.to_string())?
                .wait()
                .await
                .map_err(|e| e.to_string())?;
        }
        OpenOpts::BrowseGithub => {
            gh::browse_github(file).await?;
        }
    }
    Ok(())
}
