use crate::{
    config::Config,
    method::{LoadResp, PreviewResp},
    mode::{config_builder, ModeDef},
    nvim::{self, Neovim, NeovimExt},
    state::State,
    utils::path::to_relpath,
    utils::{bat, fzf, glow},
};

use ansi_term::ANSIGenericString;
use futures::{future::BoxFuture, FutureExt};
use once_cell::sync::Lazy;
use regex::Regex;
use rmpv::ext::from_value;
use serde::{Deserialize, Serialize};
use std::{error::Error, sync::Arc};
use tokio::sync::Mutex;

use super::CallbackMap;

#[derive(Clone)]
pub struct Diagnostics {
    items: Arc<Mutex<Option<Vec<DiagnosticsItem>>>>,
}

impl Diagnostics {
    pub fn new() -> Self {
        Self {
            items: Arc::new(Mutex::new(None)),
        }
    }
}

impl ModeDef for Diagnostics {
    fn name(&self) -> &'static str {
        "diagnostics"
    }
    fn load<'a>(
        &'a mut self,
        _config: &Config,
        state: &'a mut State,
        _query: String,
        _item: String,
    ) -> BoxFuture<'a, Result<LoadResp, String>> {
        let nvim = state.nvim.clone();
        async move {
            let mut diagnostics = DiagnosticsItem::gather(&nvim)
                .await
                .map_err(|e| e.to_string())?;
            diagnostics.sort_by(|a, b| a.severity.0.cmp(&b.severity.0));
            let items = DiagnosticsItem::render_list(&diagnostics);
            self.items.lock().await.replace(diagnostics);
            Ok(LoadResp::new_with_default_header(items))
        }
        .boxed()
    }
    fn preview<'a>(
        &'a self,
        _config: &Config,
        state: &mut State,
        item: String,
    ) -> BoxFuture<'a, Result<PreviewResp, String>> {
        let nvim = state.nvim.clone();
        async move {
            let items = self.items.lock().await;
            let items = items.as_ref().ok_or("diagnostics not loaded")?;
            let item = DiagnosticsItem::lookup(items, item.clone())?;
            let file = nvim.get_buf_name(item.bufnr as usize).await?;
            let rendered_message =
                glow::render_markdown(format!("### {}\n{}", item.severity.render(), item.message))
                    .await?;
            // zero-indexed なので +1 する
            let rendered_file =
                bat::render_file_with_highlight(&file, item.lnum as isize + 1).await?;
            let message = format!("{}\n{}", rendered_message, rendered_file);
            Ok(PreviewResp { message })
        }
        .boxed()
    }
    fn fzf_bindings(&self) -> (fzf::Bindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [{
                let self_ = self.clone();
                b.execute(move |_mode,_config,state,_query,item| {
                    let self_ = self_.clone();
                    async move {
                        let items = self_.items.lock().await;
                        let items = items.as_ref().ok_or("diagnostics not loaded")?;
                        let item = DiagnosticsItem::lookup(items, item.clone())?;
                        let opts = OpenOpts { tabedit: false };
                        open(state, item, opts).await
                    }.boxed()
                })
            }],
            "ctrl-t" => [{
                let self_ = self.clone();
                b.execute(move |_mode,_config,state,_query,item| {
                    let self_ = self_.clone();
                    async move {
                        let items = self_.items.lock().await;
                        let items = items.as_ref().ok_or("diagnostics not loaded")?;
                        let item = DiagnosticsItem::lookup(items, item.clone())?;
                        let opts = OpenOpts { tabedit: true };
                        open(state, item, opts).await
                    }.boxed()
                })
            }],
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Util
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DiagnosticsItem {
    pub bufnr: u64,
    pub file: String,
    pub lnum: u64,
    pub col: u64,
    pub message: String,
    pub severity: Severity,
}

static ITEM_PATTERN: Lazy<Regex> = Lazy::new(|| Regex::new(r".*\s{200}(?P<num>\d+)$").unwrap());

impl DiagnosticsItem {
    async fn gather(nvim: &Neovim) -> Result<Vec<DiagnosticsItem>, Box<dyn Error>> {
        Ok(from_value(
            nvim.eval_lua(
                r#"
                    local ds = vim.diagnostic.get()
                    for _, d in ipairs(ds) do
                      d.file = vim.api.nvim_buf_get_name(d.bufnr)
                    end
                    return ds
                "#,
            )
            .await?,
        )?)
    }

    fn render(&self, num: usize) -> String {
        format!(
            "{} {}|{}{}{}",
            self.severity.mark(),
            to_relpath(&self.file),
            self.message.replace('\n', ". "),
            " ".repeat(200), // numが表示から外れるように適当に長めに空白を入れる
            num,
        )
    }

    fn render_list(items: &[Self]) -> Vec<String> {
        items.iter().enumerate().map(|(i, d)| d.render(i)).collect()
    }

    fn lookup(items: &[Self], item: String) -> Result<Self, String> {
        let ix = ITEM_PATTERN
            .captures(&item)
            .and_then(|c| c.name("num"))
            .and_then(|n| n.as_str().parse::<usize>().ok())
            .ok_or("モポ")?;
        let item = items.get(ix).ok_or("モポ")?.clone();
        Ok(item)
    }
}

#[derive(Debug, Clone, serde::Deserialize, Serialize)]
pub struct Severity(pub u64);

impl Severity {
    pub fn mark(&self) -> ANSIGenericString<'_, str> {
        match self.0 {
            1 => ansi_term::Colour::Red.bold().paint("E"),
            2 => ansi_term::Colour::Yellow.bold().paint("W"),
            3 => ansi_term::Colour::Blue.bold().paint("I"),
            4 => ansi_term::Colour::White.normal().paint("H"),
            _ => panic!("unknown severity {}", self.0),
        }
    }
    pub fn render(&self) -> String {
        match self.0 {
            1 => "Error".to_string(),
            2 => "Warning".to_string(),
            3 => "Info".to_string(),
            4 => "Hint".to_string(),
            _ => panic!("unknown severity {}", self.0),
        }
    }
}

struct OpenOpts {
    tabedit: bool,
}

async fn open(state: &State, item: DiagnosticsItem, opts: OpenOpts) -> Result<(), String> {
    let nvim = state.nvim.clone();
    let file = nvim
        .get_buf_name(item.bufnr as usize)
        .await
        .map_err(|e| e.to_string())?;
    let opts = nvim::OpenOpts {
        line: Some(item.lnum as usize + 1),
        tabedit: opts.tabedit,
    };
    let _ = tokio::spawn(async move {
        let r = nvim.open(file.into(), opts).await;
        if let Err(e) = r {
            error!("diagnostics: run: nvim_open failed"; "error" => e.to_string());
        }
    })
    .await;
    Ok(())
}
