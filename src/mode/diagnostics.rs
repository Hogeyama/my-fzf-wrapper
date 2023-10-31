use crate::{
    external_command::{bat, fzf, glow},
    method::{LoadResp, PreviewResp},
    mode::{config_builder, ModeDef},
    nvim::{self, DiagnosticsItem},
    state::State,
};

use futures::{future::BoxFuture, FutureExt};
use once_cell::sync::Lazy;
use regex::Regex;
use std::error::Error;

use super::CallbackMap;

#[derive(Clone)]
pub struct Diagnostics;

impl ModeDef for Diagnostics {
    fn new() -> Self {
        Diagnostics
    }
    fn name(&self) -> &'static str {
        "diagnostics"
    }
    fn load<'a>(
        &'a mut self,
        state: &'a mut State,
        _query: String,
        _item: String,
    ) -> BoxFuture<'a, Result<LoadResp, String>> {
        let nvim = state.nvim.clone();
        async move {
            let mut diagnostics = nvim::get_all_diagnostics(&nvim)
                .await
                .map_err(|e| e.to_string())?;
            diagnostics.sort_by(|a, b| a.severity.0.cmp(&b.severity.0));
            state.keymap.insert(
                KEY_DIAGNOSTICS.to_string(), //
                json!(diagnostics),
            );
            Ok(LoadResp::new_with_default_header(to_items(diagnostics)))
        }
        .boxed()
    }
    fn preview(
        &self,
        state: &mut State,
        item: String,
    ) -> BoxFuture<'static, Result<PreviewResp, String>> {
        let nvim = state.nvim.clone();
        let diagnostics_item = match get_diagnostic_item(state, item.clone()) {
            Ok(diagnostics_item) => diagnostics_item,
            Err(e) => {
                error!("diagnostics: preview: failed"; "error" => e.to_string());
                let message = e.to_string();
                return async move { Ok(PreviewResp { message }) }.boxed();
            }
        };
        async move {
            let file = nvim::get_buf_name(&nvim, diagnostics_item.bufnr as usize)
                .await
                .map_err(|e| e.to_string());
            match file {
                Ok(file) => {
                    let rendered_message = glow::render_markdown(format!(
                        "### {}\n{}",
                        diagnostics_item.severity.render(),
                        diagnostics_item.message
                    ))
                    .await?;
                    let rendered_file =
                        // zero-indexed なので +1 する
                        bat::render_file_with_highlight(&file, diagnostics_item.lnum as isize + 1)
                            .await?;
                    let message = format!("{}\n{}", rendered_message, rendered_file);
                    Ok(PreviewResp { message })
                }
                Err(e) => Ok(PreviewResp { message: e }),
            }
        }
        .boxed()
    }
    fn fzf_bindings(&self) -> (fzf::Bindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [
                execute!(b, |_mode,state,_query,item| {
                    let opts = OpenOpts { tabedit: false };
                    open(state, item, opts).await
                })
            ],
            "ctrl-t" => [
                execute!(b, |_mode,state,_query,item| {
                    let opts = OpenOpts { tabedit: true };
                    open(state, item, opts).await
                })
            ],
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Util
////////////////////////////////////////////////////////////////////////////////////////////////////

static KEY_DIAGNOSTICS: &str = "diagnostics_diagnostics";

static ITEM_PATTERN: Lazy<Regex> = Lazy::new(|| Regex::new(r".*\s{200}(?P<num>\d+)$").unwrap());

fn to_item(_num_max_digits: usize, num: usize, d: DiagnosticsItem) -> String {
    let pwd = std::env::current_dir().unwrap();
    let pwd = pwd.to_str().unwrap();
    let relpath = d.file.as_str().replace(&format!("{pwd}/"), "");
    format!(
        "{} {}|{}{}{}",
        d.severity.mark(),
        relpath,
        d.message.replace('\n', ". "),
        " ".repeat(200), // numが表示から外れるように適当に長めに空白を入れる
        num,
    )
}

fn get_num_of_item(item: &str) -> Option<usize> {
    ITEM_PATTERN
        .captures(item)
        .and_then(|c| c.name("num"))
        .and_then(|n| n.as_str().parse::<usize>().ok())
}

fn to_items(diagnostics: Vec<DiagnosticsItem>) -> Vec<String> {
    let num_digit = diagnostics.len().to_string().len();
    diagnostics
        .into_iter()
        .enumerate()
        .map(|(i, d)| to_item(num_digit, i, d))
        .collect()
}

fn get_diagnostic_item(state: &mut State, item: String) -> Result<DiagnosticsItem, Box<dyn Error>> {
    let diagnostics: Vec<DiagnosticsItem> = serde_json::from_value(
        state
            .keymap
            .get(KEY_DIAGNOSTICS)
            .ok_or("No diagnostics yet".to_string())?
            .clone(),
    )?;
    let item_num = get_num_of_item(&item).ok_or("モポ")?;
    let diagnostics_item = diagnostics.get(item_num).ok_or("モポ")?.clone();
    Ok(diagnostics_item)
}

struct OpenOpts {
    tabedit: bool,
}

async fn open(state: &mut State, item: String, opts: OpenOpts) -> Result<(), String> {
    let nvim = state.nvim.clone();
    let diagnostics_item = get_diagnostic_item(state, item.clone()).map_err(|e| e.to_string())?;
    let file = nvim::get_buf_name(&nvim, diagnostics_item.bufnr as usize)
        .await
        .map_err(|e| e.to_string())?;
    let opts = nvim::OpenOpts {
        line: Some(diagnostics_item.lnum as usize + 1),
        tabedit: opts.tabedit,
    };
    let _ = tokio::spawn(async move {
        let r = nvim::open(&nvim, file.into(), opts).await;
        if let Err(e) = r {
            error!("diagnostics: run: nvim_open failed"; "error" => e.to_string());
        }
    });
    Ok(())
}
