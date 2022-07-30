use std::error::Error;

use crate::{
    logger::Serde,
    method::{Load, LoadResp, Method, PreviewResp, RunOpts, RunResp},
    nvim::{self, Neovim},
    types::{Mode, State},
};

use futures::{future::BoxFuture, FutureExt};
use once_cell::sync::Lazy;
use regex::Regex;
use rmpv::ext::from_value;
use serde::{Deserialize, Serialize};
use tokio::process::Command as TokioCommand;

#[derive(Clone)]
pub struct Buffer;

pub fn new() -> Buffer {
    Buffer
}

static ITEM_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"\s*(?P<bufnr>\d+):(?P<path>.*)"#).unwrap());

impl Mode for Buffer {
    fn name(&self) -> &'static str {
        "buffer"
    }
    fn load<'a>(
        &self,
        state: &'a mut State,
        _opts: Vec<String>,
    ) -> BoxFuture<'a, <Load as Method>::Response> {
        let nvim = state.nvim.clone();
        async move {
            let buffer_result = get_nvim_buffers(&nvim).await;
            match buffer_result {
                Ok(buffer_items) => {
                    let pwd = std::env::current_dir().unwrap().into_os_string();
                    LoadResp {
                        header: format!("[{}]", pwd.to_string_lossy()),
                        items: buffer_items,
                    }
                }
                Err(buffer_err) => {
                    error!("buffer.run.opts failed"; "error" => buffer_err.to_string());
                    LoadResp {
                        header: buffer_err.to_string(),
                        items: vec![],
                    }
                }
            }
        }
        .boxed()
    }
    fn preview(&self, _state: &mut State, item: String) -> BoxFuture<'static, PreviewResp> {
        async move {
            let bufnr = ITEM_PATTERN.replace(&item, "$bufnr").into_owned();
            let path = ITEM_PATTERN.replace(&item, "$path").into_owned();
            trace!("buffer: preview"; "bufnr" => bufnr, "path" => &path);
            let meta = std::fs::metadata(&path);
            match meta {
                Ok(meta) if meta.is_file() => {
                    let output = TokioCommand::new("bat")
                        .arg(&path)
                        .args(vec!["--color", "always"])
                        .output()
                        .await
                        .map_err(|e| e.to_string())
                        .expect("buffer: preview:")
                        .stdout;
                    let output = String::from_utf8_lossy(output.as_slice()).into_owned();
                    PreviewResp { message: output }
                }
                _ => {
                    trace!("buffer: preview: not a file"; "meta" => ?meta);
                    PreviewResp {
                        message: "No Preview".to_string(),
                    }
                }
            }
        }
        .boxed()
    }
    fn run<'a>(&self, state: &'a mut State, item: String, opts: RunOpts) -> BoxFuture<'a, RunResp> {
        async move {
            let bufnr = ITEM_PATTERN
                .replace(&item, "$bufnr")
                .into_owned()
                .parse::<usize>()
                .unwrap();
            let nvim = state.nvim.clone();
            let _ = tokio::spawn(async move {
                let r = nvim::open(&nvim, bufnr.into(), opts.into()).await;
                if let Err(e) = r {
                    error!("buffer: run: nvim_open failed"; "error" => e.to_string());
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

async fn get_nvim_buffers(nvim: &Neovim) -> Result<Vec<String>, Box<dyn Error>> {
    let buffers: Vec<BufferItem> = from_value(nvim.eval("getbufinfo()").await?)?;
    let mut buffers: Vec<BufferItem> = buffers
        .into_iter()
        // .filter(|b| b.name.len() > 0 && b.hidden == 0 && b.loaded == 1)
        .filter(|b| b.name.len() > 0 && b.listed > 0)
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
