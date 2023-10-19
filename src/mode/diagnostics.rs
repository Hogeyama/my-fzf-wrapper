use crate::{
    logger::Serde,
    method::{Load, LoadResp, Method, PreviewResp, RunOpts, RunResp},
    nvim::{self, DiagnosticsItem},
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
//   12|W:  5:51| unused imports: foo
static ITEM_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\s*(?P<num>\d+)\|.:\s*(?P<line>\d+):\s*(?P<col>\d+)\| (?P<message>.*)").unwrap()
});

fn to_item(
    num_max_digits: usize,
    line_max_digits: usize,
    col_max_digits: usize,
    num: usize,
    d: DiagnosticsItem,
) -> String {
    format!(
        "{:>num_width$}|{}:{:>line_width$}:{:>col_width$}| {}",
        num,
        d.severity.mark(),
        d.lnum,
        d.col,
        d.message.replace('\n', ". "),
        num_width = num_max_digits,
        line_width = line_max_digits,
        col_width = col_max_digits,
    )
}

static KEY_DIAGNOSTICS: &str = "diagnostics_diagnostics";

impl Mode for Diagnostics {
    fn name(&self) -> &'static str {
        "diagnostics"
    }
    fn load<'a>(
        &'a mut self,
        state: &'a mut State,
        _opts: Vec<String>,
    ) -> BoxFuture<'a, <Load as Method>::Response> {
        let nvim = state.nvim.clone();
        async move {
            match nvim::get_all_diagnostics(&nvim).await {
                Ok(mut diagnostics) => {
                    diagnostics.sort_by(|a, b| a.severity.0.cmp(&b.severity.0));
                    state.keymap.insert(
                        KEY_DIAGNOSTICS.to_string(), //
                        json!(diagnostics),
                    );
                    let items = to_items(diagnostics);
                    LoadResp::new_with_default_header(items)
                }
                Err(e) => {
                    error!("diagnostics: load: nvim::get_all_diagnostics failed"; "error" => e.to_string());
                    LoadResp {
                        header: e.to_string(),
                        items: vec![],
                    }
                }
            }
        }
        .boxed()
    }
    fn preview(&mut self, state: &mut State, item: String) -> BoxFuture<'static, PreviewResp> {
        let nvim = state.nvim.clone();
        let x = 21;
        let diagnostics = state.keymap.get(KEY_DIAGNOSTICS).unwrap().clone();
        let diagnostics: Vec<DiagnosticsItem> = serde_json::from_value(diagnostics).unwrap();
        let item_num = ITEM_PATTERN
            .replace(&item, "$num")
            .into_owned()
            .parse::<usize>()
            .unwrap();
        let diagnostics_item = diagnostics.get(item_num).unwrap().clone();
        info!("diagnostics: preview: diagnostics";
            "item_num" => item_num,
            "all" => Serde(diagnostics.clone()),
            "item" => Serde(diagnostics_item.clone())
        );
        async move {
            let file = nvim::get_buf_name(&nvim, diagnostics_item.bufnr as usize)
                .await
                .map_err(|e| e.to_string());
            match file {
                Ok(file) => {
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
                    let bat_output = String::from_utf8_lossy(output.as_slice()).into_owned();
                    // TODO https://github.com/charmbracelet/glow to render markdown message
                    let output = format!(
                        "\n\n{}\n\n{}",
                        diagnostics_item
                            .message
                            .lines()
                            .map(|s| format!("   {s}"))
                            .collect::<Vec<_>>()
                            .join("\n"),
                        bat_output
                    );
                    PreviewResp { message: output }
                }
                Err(e) => PreviewResp { message: e },
            }
        }
        .boxed()
    }
    fn run(
        &mut self,
        state: &mut State,
        item: String,
        opts: RunOpts,
    ) -> BoxFuture<'static, RunResp> {
        let nvim = state.nvim.clone();
        let diagnostics: Vec<DiagnosticsItem> =
            serde_json::from_value(state.keymap.get(KEY_DIAGNOSTICS).unwrap().clone()).unwrap();
        let item_num = ITEM_PATTERN
            .replace(&item, "$num")
            .into_owned()
            .parse::<usize>()
            .unwrap();
        let diagnostics_item = diagnostics.get(item_num).unwrap().clone();
        async move {
            let file = nvim::get_buf_name(&nvim, diagnostics_item.bufnr as usize)
                .await
                .unwrap();
            let line = ITEM_PATTERN.replace(&item, "$line").into_owned();
            let line = line.parse::<i64>().unwrap() + 1;
            let opts = nvim::OpenOpts {
                line: Some(line.try_into().unwrap()),
                ..opts.into()
            };
            let _ = tokio::spawn(async move {
                let r = nvim::open(&nvim, file.into(), opts).await;
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

fn to_items(diagnostics: Vec<DiagnosticsItem>) -> Vec<String> {
    let num_digit = diagnostics.len().to_string().len();
    let line_max_digits = diagnostics
        .iter()
        .map(|d| d.lnum.to_string().len())
        .max()
        .unwrap_or(0);
    let col_max_digits = diagnostics
        .iter()
        .map(|d| d.col.to_string().len())
        .max()
        .unwrap_or(0);
    let items = diagnostics
        .into_iter()
        .enumerate()
        .map(|(i, d)| to_item(num_digit, line_max_digits, col_max_digits, i, d))
        .collect();
    items
}
