use anyhow::Result;
use futures::future::BoxFuture;
use futures::FutureExt;
use git2::Status;
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
use crate::utils::fzf;
use crate::utils::fzf::PreviewWindow;
use crate::utils::gh;
use crate::utils::git;

#[derive(Clone)]
pub struct GitStatus;

impl ModeDef for GitStatus {
    fn name(&self) -> &'static str {
        "git-status"
    }
    fn load(
        &mut self,
        _config: &Config,
        _state: &mut State,
        _query: String,
        _item: String,
    ) -> super::LoadStream {
        load([
            Status::INDEX_NEW,
            Status::INDEX_MODIFIED,
            Status::WT_NEW,
            Status::WT_MODIFIED,
        ])
    }
    fn preview(
        &self,
        _config: &Config,
        _win: &PreviewWindow,
        item: String,
    ) -> BoxFuture<'static, Result<PreviewResp>> {
        preview(item)
    }
    fn fzf_bindings(&self) -> (fzf::Bindings, CallbackMap) {
        fzf_bindings()
    }
}

////////////////////////////////////////////////////////////////////////////////

fn load(statuses: impl IntoIterator<Item = Status>) -> super::LoadStream<'static> {
    let files = git::files_with_status(statuses);
    Box::pin(async_stream::stream! {
        match files {
            Ok(files) => yield Ok(LoadResp::new_with_default_header(files)),
            Err(e) => yield Err(e),
        }
    })
}

fn preview(path: String) -> BoxFuture<'static, Result<PreviewResp>> {
    async move {
        let workdir = git::workdir()?;
        let message = Command::new("git")
            .arg("diff")
            .arg("HEAD")
            .arg("--color=always")
            .arg("--no-ext")
            .arg("--")
            .arg(format!("{workdir}{path}"))
            .output()
            .await?
            .stdout;
        let message = String::from_utf8_lossy(message.as_slice()).into_owned();
        Ok(PreviewResp { message })
    }
    .boxed()
}

fn fzf_bindings() -> (fzf::Bindings, CallbackMap) {
    use config_builder::*;
    bindings! {
        b <= default_bindings(),
        "enter" => [
            execute!(b, |_mode,config,_state,_query,item| {
                let opts = OpenOpts::Neovim { tabedit: false };
                open(config, item, opts).await
            })
        ],
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
        "ctrl-space" => [
            select_and_execute!{b, |_mode,config,_state,_query,item|
                "neovim" => {
                    let opts = OpenOpts::Neovim { tabedit: false };
                    open(config, item, opts).await
                },
                "vifm" => {
                    let opts = OpenOpts::Vifm;
                    open(config, item, opts).await
                },
                "browse-github" => {
                    let opts = OpenOpts::BrowseGithub;
                    open(config, item, opts).await
                },
            }
        ]
    }
}

enum OpenOpts {
    Neovim { tabedit: bool },
    Vifm,
    BrowseGithub,
}

async fn open(config: &Config, file: String, opts: OpenOpts) -> Result<()> {
    let workdir = git::workdir()?;
    let file = format!("{}{}", workdir, file);
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
