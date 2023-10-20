use std::process::ExitStatus;

use crate::{
    external_command::{bat, rg},
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
    Lazy::new(|| Regex::new(r"(?P<file>[^:]*):(?P<line>\d+):(?P<col>\d+):.*").unwrap());

impl Mode for Rg {
    fn name(&self) -> &'static str {
        "rg"
    }
    fn load(
        &mut self,
        _state: &mut State,
        opts: Vec<String>,
    ) -> BoxFuture<'static, <Load as Method>::Response> {
        async move {
            info!("rg.load"; "opts" => Serde(opts.clone()));
            let LoadOpts { query, color } = utils::clap_parse_from::<LoadOpts>(opts).unwrap();
            info!("rg.load"; "opts" => ?rg::new().arg(query.clone()));

            let mut rg_cmd = rg::new();
            rg_cmd.arg(&format!("--color={color}"));
            rg_cmd.arg("--");
            rg_cmd.arg(query);
            info!("rg.load"; "cmd" => ?rg_cmd);
            let rg_output = rg_cmd.output().await;

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
    fn preview(&mut self, _state: &mut State, item: String) -> BoxFuture<'static, PreviewResp> {
        async move {
            let file = ITEM_PATTERN.replace(&item, "$file").into_owned();
            let line = ITEM_PATTERN.replace(&item, "$line").into_owned();
            let col = ITEM_PATTERN.replace(&item, "$col").into_owned();
            match line.parse::<isize>() {
                Ok(line) => {
                    info!("rg.preview"; "parsed" => Serde(json!({
                        "file": file,
                        "line": line,
                        "col": col
                    })));
                    let message = bat::render_file_with_highlight(&file, line).await;
                    PreviewResp { message }
                }
                Err(e) => {
                    error!("rg: preview: parse line failed"; "error" => e.to_string(), "line" => line);
                    PreviewResp {
                        message: "".to_string(),
                    }
                }
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
        info!("rg.run");
        async move {
            let file = ITEM_PATTERN.replace(&item, "$file").into_owned();
            let line = ITEM_PATTERN.replace(&item, "$line").into_owned();
            if opts.browse_github {
                let revision = TokioCommand::new("git")
                    .arg("rev-parse")
                    .arg("HEAD")
                    .output()
                    .await
                    .map_err(|e| e.to_string())
                    .expect("rg: run: git rev-parse HEAD")
                    .stdout;
                let revision = String::from_utf8_lossy(&revision).trim().to_string();
                let _: ExitStatus = TokioCommand::new("gh")
                    .arg("browse")
                    .arg(&format!("{file}:{line}"))
                    .arg(&format!("--commit={revision}"))
                    .spawn()
                    .unwrap()
                    .wait()
                    .await
                    .unwrap();
            } else {
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
            }
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
    #[clap(long, default_value = "never")]
    color: String,
    #[clap()]
    query: String,
}
