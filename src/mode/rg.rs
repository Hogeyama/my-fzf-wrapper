use crate::{
    external_command::rg,
    logger::Serde,
    method::{Load, LoadResp, Method, PreviewResp, RunOpts, RunResp},
    nvim,
    types::{Mode, State},
};

use clap::Parser;
use futures::{future::BoxFuture, FutureExt};
use once_cell::sync::Lazy;
use regex::Regex;
use tokio::process::Command as TokioCommand;

use crate::utils;

#[derive(Clone)]
pub struct Rg;

pub fn new() -> Rg {
    Rg
}

// ファイル名に colon が含まれないことを前提にしている
static ITEM_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?P<file>[^:]*):(?P<line>\d+):(?P<col>\d+):.*"#).unwrap());

impl Mode for Rg {
    fn name(&self) -> &'static str {
        "rg"
    }
    fn load<'a>(
        &self,
        state: &'a mut State,
        opts: Vec<String>,
    ) -> BoxFuture<'a, <Load as Method>::Response> {
        let _nvim = state.nvim.clone();
        async move {
            let opts = utils::clap_parse_from::<LoadOpts>(opts).unwrap();
            let rg_output = rg::new().arg(opts.query).output().await;
            match rg_output {
                Ok(rg_output) => {
                    let pwd = std::env::current_dir().unwrap().into_os_string();
                    let rg_output = String::from_utf8_lossy(&rg_output.stdout)
                        .lines()
                        .map(|line| line.to_string())
                        .collect::<Vec<_>>();
                    LoadResp {
                        header: format!("[{}]", pwd.to_string_lossy()),
                        items: rg_output,
                    }
                }
                Err(rg_err) => LoadResp {
                    header: rg_err.to_string(),
                    items: vec![],
                },
            }
        }
        .boxed()
    }
    fn preview(&self, _state: &mut State, item: String) -> BoxFuture<'static, PreviewResp> {
        async move {
            let file = ITEM_PATTERN.replace(&item, "$file").into_owned();
            let line = ITEM_PATTERN.replace(&item, "$line").into_owned();
            let col = ITEM_PATTERN.replace(&item, "$col").into_owned();
            let start_line = std::cmp::max(0, line.parse::<i64>().unwrap() - 15);
            info!("rg.preview"; "parsed" => Serde(json!({"file": file, "line": line, "col": col})));
            let output = TokioCommand::new("bat")
                .args(vec!["--color", "always"])
                .args(vec!["--line-range", &format!("{start_line}:")])
                .args(vec!["--highlight-line", &line])
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
        async move {
            let file = ITEM_PATTERN.replace(&item, "$file").into_owned();
            let line = ITEM_PATTERN.replace(&item, "$line").into_owned();
            let nvim = state.nvim.clone();
            let opts = nvim::OpenOpts {
                line: line.parse::<usize>().ok(),
                ..opts.into()
            };
            let _ = tokio::spawn(async move {
                let r = nvim::open(&nvim, file.clone().into(), opts).await;
                if let Err(e) = r {
                    error!("rg: run: nvim_open failed"; "error" => e.to_string());
                }
            });
            RunResp
        }
        .boxed()
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Load
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Parser, Debug, Clone)]
struct LoadOpts {
    #[clap()]
    query: String,
}
