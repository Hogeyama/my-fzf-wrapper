use anyhow::Result;
use futures::future::BoxFuture;
use futures::FutureExt;
use futures::StreamExt as _;
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
use crate::utils::command;
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
        &self,
        _config: &Config,
        _state: &mut State,
        _query: String,
        _item: String,
    ) -> super::LoadStream {
        Box::pin(async_stream::stream! {
            let fd = fd::new();
            let stream = command::command_output_stream(fd).chunks(100); // tekito
            tokio::pin!(stream);
            let mut has_error = false;
            while let Some(r) = stream.next().await {
                let r = r.into_iter().collect::<Result<Vec<String>>>();
                match r {
                    Ok(lines) => {
                        yield Ok(LoadResp::wip_with_default_header(lines));
                    }
                    Err(e) => {
                        yield Ok(LoadResp::error(e.to_string()));
                        has_error = true;
                        break;
                    }
                }
            }
            if !has_error {
                yield Ok(LoadResp::last())
            }
        })
    }
    fn preview(
        &self,
        _config: &Config,
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
                execute!(b, |_mode,config,_state,_query,item| {
                    let opts = OpenOpts::Neovim { tabedit: false };
                    open(config, item, opts).await
                })
            ],
            "ctrl-t" => [
                execute!(b, |_mode,config,_state,_query,item| {
                    let opts = OpenOpts::Neovim { tabedit: true };
                    open(config, item, opts).await
                })
            ],
            "ctrl-v" => [
                execute!(b, |_mode,config,_state,_query,item| {
                    let opts = OpenOpts::Vifm;
                    open(config, item, opts).await
                })
            ],
            "ctrl-y" => [
                execute!(b, |_mode,_config,_state,_query,item| {
                    xsel::yank(item).await?;
                    Ok(())
                })
            ],
            "ctrl-space" => [
                select_and_execute!{b, |_mode,config,_state,_query,item|
                    "new file" => {
                        let cwd = std::env::current_dir().unwrap();
                        let fname = fzf::input_with_placeholder("Enter file name", &item).await?;
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
                        open(config, path, opts).await
                    },
                    "execute any command" => {
                        let (cmd, output) = edit_and_run(format!(" {item}"))
                            .await?;
                        config.nvim.notify_command_result(&cmd, output)
                            .await?;
                        Ok(())
                    },
                    "browse-github" => {
                        let opts = OpenOpts::BrowseGithub;
                        open(config, item, opts).await
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

async fn open(config: &Config, file: String, opts: OpenOpts) -> Result<()> {
    match opts {
        OpenOpts::Neovim { tabedit } => {
            let nvim = config.nvim.clone();
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
