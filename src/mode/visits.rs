use anyhow::Result;
use futures::future::BoxFuture;
use futures::stream;
use futures::stream::StreamExt;
use futures::FutureExt;
use rmpv::ext::from_value;

use super::lib::actions;
use super::lib::item::FilePathItem;
use crate::env::Env;
use crate::method::LoadResp;
use crate::method::PreviewResp;
use crate::mode::config_builder;
use crate::mode::CallbackMap;
use crate::mode::ModeDef;
use crate::nvim::Neovim;
use crate::nvim::NeovimExt;
use crate::utils::bat;
use crate::utils::fzf::PreviewWindow;
use crate::utils::path::to_relpath;

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
    fn load(&self, env: &Env, _query: String, _item: String) -> super::LoadStream {
        let nvim = env.nvim.clone();
        let kind = self.kind;
        Box::pin(async_stream::stream! {
            let mru_items = get_visits(&nvim, kind).await?;
            yield Ok(LoadResp::new_with_default_header(mru_items))
        })
    }
    fn preview(
        &self,
        _env: &Env,
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
    fn fzf_bindings(&self) -> (super::ModeBindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [ b.open_nvim(FilePathItem, false) ],
            "ctrl-t" => [ b.open_nvim(FilePathItem, true) ],
            "ctrl-y" => [ b.yank_file(FilePathItem) ],
            "ctrl-x" => [
                execute_silent!(b, |_mode,env,_query,item| {
                    env.nvim.eval_lua(
                        format!("require'mini.visits'.remove_path('{}')", item)
                    ).await?;
                    Ok(())
                }),
                b.reload(),
            ],
            "pgup" => [
                select_and_execute!{b, |_mode,env,_query,item|
                    "oil" => {
                        actions::oil(env).await
                    },
                    "new file" => {
                        actions::new_file(env, &item).await
                    },
                    "execute any command" => {
                        actions::execute_command(env, &item).await
                    },
                    "vscode" => {
                        actions::open_in_vscode(env, item, None).await
                    },
                }
            ]
        }
    }
    fn wants_sort(&self) -> bool {
        false
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
