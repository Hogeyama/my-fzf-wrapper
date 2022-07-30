use std::error::Error;

use crate::{
    logger::Serde,
    method::{Load, LoadResp, Method, PreviewResp, RunOpts, RunResp},
    nvim::{self, Neovim},
    types::{Mode, State},
};

use futures::stream::{self, StreamExt};
use futures::{future::BoxFuture, FutureExt};
use once_cell::sync::Lazy;
use regex::Regex;
use rmpv::ext::from_value;
use serde::{Deserialize, Serialize};
use tokio::process::Command as TokioCommand;

#[derive(Clone)]
pub struct Mru;

pub fn new() -> Mru {
    Mru
}

static ITEM_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"\s*(?P<bufnr>\d+):(?P<path>.*)"#).unwrap());

impl Mode for Mru {
    fn name(&self) -> &'static str {
        "mru"
    }
    fn load<'a>(
        &mut self,
        state: &'a mut State,
        _opts: Vec<String>,
    ) -> BoxFuture<'a, <Load as Method>::Response> {
        let nvim = state.nvim.clone();
        async move {
            let mru_result = get_nvim_oldefiles(&nvim).await;
            match mru_result {
                Ok(mru_items) => {
                    let pwd = std::env::current_dir().unwrap().into_os_string();
                    LoadResp {
                        header: format!("[{}]", pwd.to_string_lossy()),
                        items: mru_items,
                    }
                }
                Err(mru_err) => {
                    error!("mru.run.opts failed"; "error" => mru_err.to_string());
                    LoadResp {
                        header: mru_err.to_string(),
                        items: vec![],
                    }
                }
            }
        }
        .boxed()
    }
    fn preview(&mut self, _state: &mut State, item: String) -> BoxFuture<'static, PreviewResp> {
        async move {
            let bufnr = ITEM_PATTERN.replace(&item, "$bufnr").into_owned();
            let path = ITEM_PATTERN.replace(&item, "$path").into_owned();
            trace!("mru: preview"; "bufnr" => bufnr, "path" => &path);
            let meta = std::fs::metadata(&path);
            match meta {
                Ok(meta) if meta.is_file() => {
                    let output = TokioCommand::new("bat")
                        .arg(&path)
                        .args(vec!["--color", "always"])
                        .output()
                        .await
                        .map_err(|e| e.to_string())
                        .expect("mru: preview:")
                        .stdout;
                    let output = String::from_utf8_lossy(output.as_slice()).into_owned();
                    PreviewResp { message: output }
                }
                _ => {
                    trace!("mru: preview: not a file"; "meta" => ?meta);
                    PreviewResp {
                        message: "No Preview".to_string(),
                    }
                }
            }
        }
        .boxed()
    }
    fn run<'a>(
        &mut self,
        state: &'a mut State,
        item: String,
        opts: RunOpts,
    ) -> BoxFuture<'a, RunResp> {
        async move {
            let bufnr = ITEM_PATTERN.replace(&item, "$bufnr").into_owned();
            let nvim = state.nvim.clone();
            let _ = tokio::spawn(async move {
                let r = nvim::open(&nvim, bufnr.into(), opts.into()).await;
                if let Err(e) = r {
                    error!("mru: run: nvim_open failed"; "error" => e.to_string());
                }
            });
            RunResp
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
