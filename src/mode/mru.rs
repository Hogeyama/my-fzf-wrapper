use std::error::Error;

use crate::{
    external_command::bat,
    logger::Serde,
    method::{LoadResp, PreviewResp, RunOpts, RunResp},
    nvim::{self, Neovim},
    types::{Mode, State},
};

use futures::stream::{self, StreamExt};
use futures::{future::BoxFuture, FutureExt};
use once_cell::sync::Lazy;
use regex::Regex;
use rmpv::ext::from_value;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct Mru;

static ITEM_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\s*(?P<bufnr>\d+):(?P<path>.*)").unwrap());

impl Mode for Mru {
    fn new() -> Self {
        Mru
    }
    fn name(&self) -> &'static str {
        "mru"
    }
    fn load(
        &self,
        state: &mut State,
        _opts: Vec<String>,
    ) -> BoxFuture<'static, Result<LoadResp, String>> {
        let nvim = state.nvim.clone();
        async move {
            let mru_items = get_nvim_oldefiles(&nvim).await.map_err(|e| e.to_string())?;
            Ok(LoadResp::new_with_default_header(mru_items))
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
    fn run(
        &self,
        state: &mut State,
        item: String,
        opts: RunOpts,
    ) -> BoxFuture<'static, Result<RunResp, String>> {
        let nvim = state.nvim.clone();
        async move {
            let bufnr = ITEM_PATTERN.replace(&item, "$bufnr").into_owned();
            let _ = tokio::spawn(async move {
                let r = nvim::open(&nvim, bufnr.into(), opts.into()).await;
                if let Err(e) = r {
                    error!("mru: run: nvim_open failed"; "error" => e.to_string());
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

async fn is_file(path: String) -> bool {
    let meta = tokio::fs::metadata(path).await;
    match meta {
        Ok(meta) if meta.is_file() => true,
        _ => false,
    }
}

async fn get_nvim_oldefiles(nvim: &Neovim) -> Result<Vec<String>, Box<dyn Error>> {
    let mrus: Vec<String> = from_value(nvim.eval("v:oldfiles").await?)?;
    // TODO clone が必要な理由
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
