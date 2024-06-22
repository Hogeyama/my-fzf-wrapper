use crate::{
    config::Config,
    method::{LoadResp, PreviewResp},
    mode::{config_builder, ModeDef},
    nvim::{self, Neovim, NeovimExt},
    state::State,
    utils::{
        bat,
        command::edit_and_run,
        fzf::{self, PreviewWindow},
        path::to_relpath,
        xsel,
    },
};

use anyhow::Result;
use futures::stream::{self, StreamExt};
use futures::{future::BoxFuture, FutureExt};
use rmpv::ext::from_value;

use super::CallbackMap;

#[derive(Clone)]
pub struct Visits {
    kind: VisitsKind,
}

#[derive(Clone, Copy)]
pub enum VisitsKind {
    All,
    Project,
}

impl Visits {
    pub fn new(kind: VisitsKind) -> Self {
        Self { kind }
    }
    pub fn all() -> Self {
        Self::new(VisitsKind::All)
    }
    pub fn project() -> Self {
        Self::new(VisitsKind::Project)
    }
}

impl ModeDef for Visits {
    fn name(&self) -> &'static str {
        match self.kind {
            VisitsKind::All => "visits:all",
            VisitsKind::Project => "visists:cwd",
        }
    }
    fn load(
        &mut self,
        _config: &Config,
        state: &mut State,
        _query: String,
        _item: String,
    ) -> BoxFuture<'static, Result<LoadResp>> {
        let nvim = state.nvim.clone();
        let kind = self.kind;
        async move {
            let mru_items = get_visits(&nvim, kind).await?;
            Ok(LoadResp::new_with_default_header(mru_items))
        }
        .boxed()
    }
    fn preview(
        &self,
        _config: &Config,
        _state: &mut State,
        _win: &PreviewWindow,
        item: String,
    ) -> BoxFuture<'static, Result<PreviewResp>> {
        async move {
            let meta = std::fs::metadata(&item);
            match meta {
                Ok(meta) if meta.is_file() => {
                    let message = bat::render_file(&item).await?;
                    Ok(PreviewResp { message })
                }
                _ => Ok(PreviewResp {
                    message: "No Preview".to_string(),
                }),
            }
        }
        .boxed()
    }
    fn fzf_bindings(&self) -> (fzf::Bindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [
                execute!(b, |_mode,_config,state,_query,item| {
                    let opts = OpenOpts { tabedit: false };
                    open(state, item, opts).await
                })
            ],
            "ctrl-t" => [
                execute!(b, |_mode,_config,state,_query,item| {
                    let opts = OpenOpts { tabedit: true };
                    open(state, item, opts).await
                })
            ],
            "ctrl-y" => [
                execute!(b, |_mode,_config,_state,_query,item| {
                    xsel::yank(item).await?;
                    Ok(())
                })
            ],
            "ctrl-x" => [
                execute_silent!(b, |_mode,_config,state,_query,item| {
                    state.nvim.eval_lua(
                        format!("require'mini.visits'.remove_path('{}')", item)
                    ).await?;
                    Ok(())
                }),
                b.reload(),
            ],
            "ctrl-space" => [
                select_and_execute!{b, |_mode,_config,state,_query,item|
                    "execute any command" => {
                        let (cmd, output) = edit_and_run(format!(" {item}"))
                            .await?;
                        state.nvim.notify_command_result(&cmd, output)
                            .await?;
                        Ok(())
                    },
                }
            ]
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Util
////////////////////////////////////////////////////////////////////////////////////////////////////

async fn is_file(path: String) -> bool {
    let meta = tokio::fs::metadata(path).await;
    matches!(meta, Ok(meta) if meta.is_file())
}

async fn get_visits(nvim: &Neovim, kind: VisitsKind) -> Result<Vec<String>> {
    let mrus: Vec<String> = from_value(
        nvim.eval_lua(format!(
            "return require'mini.visits'.list_paths({})",
            match kind {
                VisitsKind::All => "''",      // empty string for all project
                VisitsKind::Project => "nil", // nil for current project
            }
        ))
        .await?,
    )?;
    let mrus = stream::iter(mrus)
        .filter(|x| is_file(x.clone()))
        .map(to_relpath)
        .collect::<Vec<_>>()
        .await;
    Ok(mrus)
}

struct OpenOpts {
    tabedit: bool,
}

async fn open(state: &mut State, item: String, opts: OpenOpts) -> Result<()> {
    let OpenOpts { tabedit } = opts;
    let nvim = state.nvim.clone();
    let nvim_opts = nvim::OpenOpts {
        line: None,
        tabedit,
    };
    nvim.open(item.into(), nvim_opts).await?;
    Ok(())
}
