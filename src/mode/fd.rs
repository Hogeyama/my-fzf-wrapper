use anyhow::Result;
use futures::future::BoxFuture;
use futures::FutureExt;
use tokio::process::Command;

use crate::config::Config;
use crate::method::LoadResp;
use crate::method::PreviewResp;
use crate::mode::config_builder;
use crate::mode::CallbackMap;
use crate::mode::ModeDef;
use crate::nvim;
use crate::nvim::NeovimExt;
use crate::state::State;
use crate::utils::bat;
use crate::utils::command::edit_and_run;
use crate::utils::fd;
use crate::utils::fzf;
use crate::utils::fzf::PreviewWindow;
use crate::utils::gh;
use crate::utils::xsel;

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
    ) -> super::LoadStream {
        Box::pin(async_stream::stream! {
            let fd_output = fd::new().output().await?;
            let fd_output = String::from_utf8_lossy(&fd_output.stdout)
                .lines()
                .map(|line| line.to_string())
                .collect::<Vec<_>>();
            yield Ok(LoadResp::new_with_default_header(fd_output))
        })
    }
    fn preview(
        &self,
        _config: &Config,
        _state: &mut State,
        _win: &PreviewWindow,
        item: String,
    ) -> BoxFuture<'static, Result<PreviewResp>> {
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
                    "new file" => {
                        let cwd = std::env::current_dir().unwrap();
                        let fname = fzf::input("Enter file name").await?;
                        let fname = fname.trim();
                        let path = format!("{}/{}", cwd.display(), fname);
                        let dir = std::path::Path::new(&path).parent().unwrap();
                        Command::new("mkdir")
                            .arg("-p")
                            .arg(dir)
                            .status()
                            .await?;
                        Command::new("touch")
                            .arg(&path)
                            .status()
                            .await?;
                        let opts = OpenOpts::Neovim { tabedit: false };
                        open(state, path, opts).await
                    },
                    "execute any command" => {
                        let (cmd, output) = edit_and_run(format!(" {item}"))
                            .await?;
                        state.nvim.notify_command_result(&cmd, output)
                            .await?;
                        Ok(())
                    },
                    "browse-github" => {
                        let opts = OpenOpts::BrowseGithub;
                        open(state, item, opts).await
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

async fn open(state: &mut State, file: String, opts: OpenOpts) -> Result<()> {
    match opts {
        OpenOpts::Neovim { tabedit } => {
            let nvim = state.nvim.clone();
            let nvim_opts = nvim::OpenOpts {
                line: None,
                tabedit,
            };
            nvim.open(file.into(), nvim_opts).await?
        }
        OpenOpts::Vifm => {
            let pwd = std::env::current_dir().unwrap().into_os_string();
            Command::new("vifm").arg(&pwd).spawn()?.wait().await?;
        }
        OpenOpts::BrowseGithub => {
            gh::browse_github(file).await?;
        }
    }
    Ok(())
}
