use anyhow::Result;
use futures::future::BoxFuture;
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
pub struct Buffer;

static ITEM_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\s*(?P<bufnr>\d+):(?P<path>.*)").unwrap());

impl ModeDef for Buffer {
    fn name(&self) -> &'static str {
        "buffer"
    }
    fn load(
        &mut self,
        config: &Config,
        _state: &mut State,
        _query: String,
        _item: String,
    ) -> super::LoadStream {
        let nvim = config.nvim.clone();
        Box::pin(async_stream::stream! {
            let items = get_nvim_buffers(&nvim).await?;
            yield Ok(LoadResp::new_with_default_header(items))
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
            let bufnr = ITEM_PATTERN.replace(&item, "$bufnr").into_owned();
            let path = ITEM_PATTERN.replace(&item, "$path").into_owned();
            trace!("buffer: preview"; "bufnr" => bufnr, "path" => &path);
            let meta = std::fs::metadata(&path);
            match meta {
                Ok(meta) if meta.is_file() => {
                    let message = bat::render_file(&path).await?;
                    Ok(PreviewResp { message })
                }
                _ => {
                    trace!("buffer: preview: not a file"; "meta" => ?meta);
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
                execute!(b, |_mode,config,_state,_query,item| {
                    let opts = ExecOpts::Open { tabedit: false };
                    exec(config, item, opts).await
                })
            ],
            "ctrl-t" => [
                execute!(b, |_mode,config,_state,_query,item| {
                    let opts = ExecOpts::Open { tabedit: true };
                    exec(config, item, opts).await
                })
            ],
            "ctrl-x" => [
                execute!(b, |_mode,config,_state,_query,item| {
                    let opts = ExecOpts::Delete { force: true };
                    exec(config, item, opts).await
                }),
                b.reload(),
            ],
            "ctrl-y" => [
                execute!(b, |_mode,_config,_state,_query,item| {
                    let file = ITEM_PATTERN.replace(&item, "$path");
                    xsel::yank(file).await?;
                    Ok(())
                })
            ],
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Util
////////////////////////////////////////////////////////////////////////////////////////////////////

async fn get_nvim_buffers(nvim: &Neovim) -> Result<Vec<String>> {
    let buffers: Vec<BufferItem> = from_value(nvim.eval("getbufinfo()").await?)?;
    let mut buffers: Vec<BufferItem> = buffers
        .into_iter()
        // .filter(|b| b.name.len() > 0 && b.hidden == 0 && b.loaded == 1)
        .filter(|b| !b.name.is_empty() && b.listed > 0)
        .collect();
    buffers.sort_by(|a, b| b.lastused.cmp(&a.lastused));
    trace!("buffer: get_nvim_buffers: buffers"; "buffers" => Serde(buffers.clone()));
    let items = buffers
        .into_iter()
        .map(|b| format!("{:>3}:{}", b.bufnr, b.name))
        .collect();
    Ok(items)
}

// :h getbufinfo() から抜粋
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BufferItem {
    bufnr: u64,
    name: String,
    lnum: u64,
    lastused: u64,
    listed: u64,
    hidden: u64,
    loaded: u64,
}

enum ExecOpts {
    Open { tabedit: bool },
    Delete { force: bool },
}

async fn exec(config: &Config, item: String, opts: ExecOpts) -> Result<()> {
    let bufnr = ITEM_PATTERN
        .replace(&item, "$bufnr")
        .into_owned()
        .parse::<usize>()?;
    match opts {
        ExecOpts::Open { tabedit } => {
            let nvim = config.nvim.clone();
            let nvim_opts = nvim::OpenOpts {
                line: None,
                tabedit,
            };
            let r = nvim.open(bufnr.into(), nvim_opts).await;
            if let Err(e) = r {
                error!("buffer: run: nvim_open failed"; "error" => e.to_string());
            }
        }
        ExecOpts::Delete { force } => {
            let nvim = config.nvim.clone();
            let r = nvim.delete_buffer(bufnr, force).await;
            if let Err(e) = r {
                error!("buffer: run: nvim_delete_buffer failed"; "error" => e.to_string());
            }
        }
    }
    Ok(())
}
