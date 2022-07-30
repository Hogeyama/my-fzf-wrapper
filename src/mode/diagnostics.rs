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
use tokio::process::Command as TokioCommand;

#[derive(Clone)]
pub struct Diagnostics;

pub fn new() -> Diagnostics {
    Diagnostics
}

// example:
// W:  5:51| unused imports: foo
static ITEM_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#".:\s*(?P<line>\d+):\s*(?P<col>\d+)\| (?P<message>.*)"#).unwrap());

impl Mode for Diagnostics {
    fn name(&self) -> &'static str {
        "diagnostics"
    }
    fn load<'a>(
        &self,
        state: &'a mut State,
        _opts: Vec<String>,
    ) -> BoxFuture<'a, <Load as Method>::Response> {
        let nvim = state.nvim.clone();
        async move {
            let diagnostics_result = get_nvim_diagnostics(&nvim).await;
            match diagnostics_result {
                Ok(diagnostics_items) => {
                    let pwd = std::env::current_dir().unwrap().into_os_string();
                    LoadResp {
                        header: format!("[{}]", pwd.to_string_lossy()),
                        items: diagnostics_items,
                    }
                }
                Err(diagnostics_err) => {
                    error!("diagnostics.run.opts failed"; "error" => diagnostics_err.to_string());
                    LoadResp {
                        header: diagnostics_err.to_string(),
                        items: vec![],
                    }
                }
            }
        }
        .boxed()
    }
    fn preview(&self, state: &mut State, item: String) -> BoxFuture<'static, PreviewResp> {
        let nvim = state.nvim.clone();
        async move {
            // TODO 表示してから neovim を操作すると変わってしまう。保存しておく必要がある。
            let file = nvim::last_opened_file(&nvim)
                .await
                .unwrap_or("".to_string());
            let line = ITEM_PATTERN.replace(&item, "$line").into_owned();
            let line = line.parse::<i64>().unwrap() + 1;
            let start_line = std::cmp::max(0, line - 15);
            let output = TokioCommand::new("bat")
                .args(vec!["--color", "always"])
                .args(vec!["--line-range", &format!("{start_line}:")])
                .args(vec!["--highlight-line", &line.to_string()])
                .arg(&file)
                .output()
                .await
                .map_err(|e| e.to_string())
                .expect("rg: preview:")
                .stdout;
            let output = String::from_utf8_lossy(output.as_slice()).into_owned();
            PreviewResp { message: output }
        }
        .boxed()
    }
    fn run<'a>(&self, state: &'a mut State, item: String, opts: RunOpts) -> BoxFuture<'a, RunResp> {
        let nvim = state.nvim.clone();
        async move {
            let file = nvim::last_opened_file(&nvim)
                .await
                .unwrap_or("".to_string());
            let line = ITEM_PATTERN.replace(&item, "$line").into_owned();
            let line = line.parse::<i64>().unwrap() + 1;
            let opts = nvim::OpenOpts {
                line: Some(line.try_into().unwrap()),
                ..opts.into()
            };
            let _ = tokio::spawn(async move {
                let r = nvim::open(&nvim, file.into(), opts.into()).await;
                if let Err(e) = r {
                    error!("diagnostics: run: nvim_open failed"; "error" => e.to_string());
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

async fn get_nvim_diagnostics(nvim: &Neovim) -> Result<Vec<String>, Box<dyn Error>> {
    let mut diagnosticss: Vec<nvim::DiagnosticsItem> = nvim::get_buf_diagnostics(nvim).await?;
    diagnosticss.sort_by(|a, b| b.severity.0.cmp(&a.severity.0));
    info!("diagnostics: get_nvim_diagnostics"; "diagnosticss" => Serde(diagnosticss.clone()));
    let line_max_digits = diagnosticss
        .iter()
        .map(|d| d.lnum.to_string().len())
        .max()
        .unwrap_or(0);
    let col_max_digits = diagnosticss
        .iter()
        .map(|d| d.col.to_string().len())
        .max()
        .unwrap_or(0);
    let items = diagnosticss
        .into_iter()
        .map(|d| {
            format!(
                "{}:{:>line_width$}:{:>col_width$}| {}",
                d.severity.mark(),
                d.lnum,
                d.col,
                d.message.replace("\n", ". "),
                line_width = line_max_digits,
                col_width = col_max_digits,
            )
        })
        .collect();
    Ok(items)
}
