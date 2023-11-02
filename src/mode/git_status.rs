use futures::{future::BoxFuture, FutureExt};
use git2::Status;
use tokio::process::Command;

use crate::{
    config::Config,
    external_command::{fzf, gh, git},
    method::{LoadResp, PreviewResp},
    mode::{config_builder, ModeDef},
    nvim,
    state::State,
};

use super::CallbackMap;

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
    ) -> BoxFuture<'static, Result<LoadResp, String>> {
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
        _state: &mut State,
        item: String,
    ) -> BoxFuture<'static, Result<PreviewResp, String>> {
        preview(item)
    }
    fn fzf_bindings(&self) -> (fzf::Bindings, CallbackMap) {
        fzf_bindings()
    }
}

////////////////////////////////////////////////////////////////////////////////

fn load(
    statuses: impl IntoIterator<Item = Status>,
) -> BoxFuture<'static, Result<LoadResp, String>> {
    let files = git::files_with_status(statuses);
    async move {
        match files {
            Ok(files) => Ok(LoadResp::new_with_default_header(files)),
            Err(e) => Err(e),
        }
    }
    .boxed()
}

fn preview(path: String) -> BoxFuture<'static, Result<PreviewResp, String>> {
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
            .await
            .map_err(|e| e.to_string())?
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
            execute!(b, |_mode,_config,state,_query,item| {
                let opts = OpenOpts::Neovim { tabedit: false };
                open(state, item, opts).await
            })
        ],
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
            }
        ]
    }
}

enum OpenOpts {
    Neovim { tabedit: bool },
    Vifm,
    BrowseGithub,
}

async fn open(state: &mut State, file: String, opts: OpenOpts) -> Result<(), String> {
    let workdir = git::workdir()?;
    let file = format!("{}{}", workdir, file);
    match opts {
        OpenOpts::Neovim { tabedit } => {
            let nvim = state.nvim.clone();
            let nvim_opts = nvim::OpenOpts {
                line: None,
                tabedit,
            };
            nvim::open(&nvim, file.into(), nvim_opts)
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
