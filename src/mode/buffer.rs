use std::error::Error;

use crate::{
    external_command::bat,
    logger::Serde,
    method::{LoadResp, PreviewResp, RunOpts, RunResp},
    nvim::{self, Neovim},
    types::{Mode, State},
};

use futures::{future::BoxFuture, FutureExt};
use once_cell::sync::Lazy;
use regex::Regex;
use rmpv::ext::from_value;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct Buffer;

static ITEM_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\s*(?P<bufnr>\d+):(?P<path>.*)").unwrap());

impl Mode for Buffer {
    fn new() -> Self {
        Buffer
    }
    fn name(&self) -> &'static str {
        "buffer"
    }
    fn load(
        &self,
        state: &mut State,
        _opts: Vec<String>,
    ) -> BoxFuture<'static, Result<LoadResp, String>> {
        let nvim = state.nvim.clone();
        async move {
            let items = get_nvim_buffers(&nvim).await.map_err(|e| e.to_string())?;
            Ok(LoadResp::new_with_default_header(items))
        }
        .boxed()
    }
    fn preview(
        &self,
        _state: &mut State,
        item: String,
    ) -> BoxFuture<'static, Result<PreviewResp, String>> {
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
    fn run(
        &self,
        state: &mut State,
        item: String,
        opts: RunOpts,
    ) -> BoxFuture<'static, Result<RunResp, String>> {
        let nvim = state.nvim.clone();
        async move {
            let bufnr = ITEM_PATTERN
                .replace(&item, "$bufnr")
                .into_owned()
                .parse::<usize>()
                .map_err(|e| e.to_string())?;
            let _ = tokio::spawn(async move {
                if opts.delete {
                    let r = nvim::delete_buffer(&nvim, bufnr, opts.force).await;
                    if let Err(e) = r {
                        error!("buffer: run: nvim_delete_buffer failed"; "error" => e.to_string());
                    }
                } else {
                    let r = nvim::open(&nvim, bufnr.into(), opts.into()).await;
                    if let Err(e) = r {
                        error!("buffer: run: nvim_open failed"; "error" => e.to_string());
                    }
                }
            });
            Ok(RunResp)
        }
        .boxed()
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Util
////////////////////////////////////////////////////////////////////////////////////////////////////

async fn get_nvim_buffers(nvim: &Neovim) -> Result<Vec<String>, Box<dyn Error>> {
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
