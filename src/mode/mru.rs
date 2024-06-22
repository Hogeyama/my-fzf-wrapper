use anyhow::Result;
use futures::future::BoxFuture;
use futures::stream;
use futures::stream::StreamExt;
use futures::FutureExt;
use once_cell::sync::Lazy;
use regex::Regex;
use rmpv::ext::from_value;
use serde::Deserialize;
use serde::Serialize;

use crate::config::Config;
use crate::logger::Serde;
use crate::method::LoadResp;
use crate::method::PreviewResp;
use crate::mode::config_builder;
use crate::mode::CallbackMap;
use crate::mode::ModeDef;
use crate::nvim;
use crate::nvim::Neovim;
use crate::nvim::NeovimExt;
use crate::state::State;
use crate::utils::bat;
use crate::utils::fzf;
use crate::utils::fzf::PreviewWindow;
use crate::utils::xsel;

#[derive(Clone)]
pub struct Mru;

static ITEM_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\s*(?P<bufnr>\d+):(?P<path>.*)").unwrap());

impl ModeDef for Mru {
    fn name(&self) -> &'static str {
        "mru"
    }
    fn load(
        &mut self,
        _config: &Config,
        state: &mut State,
        _query: String,
        _item: String,
    ) -> BoxFuture<'static, Result<LoadResp>> {
        let nvim = state.nvim.clone();
        async move {
            let mru_items = get_nvim_oldefiles(&nvim).await?;
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
            let bufnr = ITEM_PATTERN.replace(&item, "$bufnr").into_owned();
            let path = ITEM_PATTERN.replace(&item, "$path").into_owned();
            trace!("mru: preview"; "bufnr" => bufnr, "path" => &path);
            let meta = std::fs::metadata(&path);
            match meta {
                Ok(meta) if meta.is_file() => {
                    let message = bat::render_file(&item).await?;
                    Ok(PreviewResp { message })
                }
                _ => {
                    trace!("mru: preview: not a file"; "meta" => ?meta);
                    Ok(PreviewResp {
                        message: "No Preview".to_string(),
                    })
                }
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

async fn get_nvim_oldefiles(nvim: &Neovim) -> Result<Vec<String>> {
    let mrus: Vec<String> = from_value(nvim.eval("v:oldfiles").await?)?;
    let mrus = stream::iter(mrus)
        .filter(|x| is_file(x.clone()))
        .collect::<Vec<_>>()
        .await;
    info!("mru: get_nvim_oldefiles: mrus"; "mrus" => Serde(mrus.clone()));
    Ok(mrus)
}

// :h getbufinfo() から抜粋
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MruItem {
    bufnr: u64,
    name: String,
    lnum: u64,
    lastused: u64,
    listed: u64,
    hidden: u64,
    loaded: u64,
}

struct OpenOpts {
    tabedit: bool,
}

async fn open(state: &mut State, item: String, opts: OpenOpts) -> Result<()> {
    let bufnr = ITEM_PATTERN.replace(&item, "$bufnr").into_owned();
    let OpenOpts { tabedit } = opts;
    let nvim = state.nvim.clone();
    let nvim_opts = nvim::OpenOpts {
        line: None,
        tabedit,
    };
    nvim.open(bufnr.into(), nvim_opts).await?;
    Ok(())
}
