use anyhow::Result;
use clap::Parser;
use futures::future::BoxFuture;
use futures::FutureExt;
use once_cell::sync::Lazy;
use regex::Regex;

use crate::config::Config;
use crate::logger::Serde;
use crate::method::LoadResp;
use crate::method::PreviewResp;
use crate::mode::config_builder;
use crate::mode::CallbackMap;
use crate::mode::ModeDef;
use crate::nvim;
use crate::nvim::NeovimExt;
use crate::state::State;
use crate::utils::bat;
use crate::utils::fzf;
use crate::utils::fzf::PreviewWindow;
use crate::utils::gh;
use crate::utils::git;
use crate::utils::rg;

////////////////////////////////////////////////////////////////////////////////
// Livegrep
////////////////////////////////////////////////////////////////////////////////

#[derive(Clone)]
pub struct LiveGrep {
    name: &'static str,
    rg_opts: Vec<String>,
}

impl LiveGrep {
    pub fn new() -> Self {
        Self {
            name: "livegrep",
            rg_opts: vec!["--glob".to_string(), "!.git".to_string()],
        }
    }
    pub fn new_no_ignore() -> Self {
        Self {
            name: "livegrep(no-ignore)",
            rg_opts: vec!["--no-ignore".to_string()],
        }
    }
}

impl ModeDef for LiveGrep {
    fn name(&self) -> &'static str {
        self.name
    }
    fn load(
        &mut self,
        _config: &Config,
        _state: &mut State,
        query: String,
        _item: String,
    ) -> super::LoadStream {
        load(query, &self.rg_opts)
    }
    fn preview(
        &self,
        _config: &Config,
        _state: &mut State,
        _win: &PreviewWindow,
        item: String,
    ) -> BoxFuture<'static, Result<PreviewResp>> {
        async move { preview(item).await }.boxed()
    }
    fn fzf_bindings(&self) -> (fzf::Bindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "change" => [
                b.reload(),
            ],
            "ctrl-c" => [
                b.change_mode(LiveGrepF.name(), false),
            ],
            "esc" => [
                b.change_mode(LiveGrepF.name(), false),
            ],
            "enter" => [
                execute!(b, |_mode,_config,state,_query,item| {
                    let opts = OpenOpts::Neovim { tabedit: false };
                    open(state, item, opts).await
                })
            ],
            "ctrl-t" => [
                execute!(b, |_mode,_config,state,_query,item| {
                    let opts = OpenOpts::Neovim { tabedit: true };
                    open(state, item, opts).await
                })
            ],
            "ctrl-space" => [
                select_and_execute!{b, |_mode,_config,state,_query,item|
                    "neovim" => {
                        let opts = OpenOpts::Neovim { tabedit: false };
                        open(state, item, opts).await
                    },
                    "browse-github" => {
                        let opts = OpenOpts::BrowseGithub;
                        open(state, item, opts).await
                    },
                }
            ]
        }
    }
    fn fzf_extra_opts(&self) -> Vec<&str> {
        vec!["--disabled"]
    }
}

#[derive(Parser, Debug, Clone)]
pub struct LoadOpts {
    #[clap()]
    pub query: String,
}

fn load(query: String, opts: &Vec<String>) -> super::LoadStream {
    let mut rg_cmd = rg::new();
    rg_cmd.args(opts);
    rg_cmd.arg("--");
    rg_cmd.arg(query);
    Box::pin(async_stream::stream! {
        let rg_output = rg_cmd.output().await?;
        let rg_output = String::from_utf8_lossy(&rg_output.stdout)
            .lines()
            .map(|line| line.to_string())
            .collect::<Vec<_>>();
        yield Ok(LoadResp::new_with_default_header(rg_output))
    })
}

////////////////////////////////////////////////////////////////////////////////
// Fuzzy search after livegrep
////////////////////////////////////////////////////////////////////////////////

#[derive(Clone)]
pub struct LiveGrepF;

impl ModeDef for LiveGrepF {
    fn name(&self) -> &'static str {
        "livegrepf"
    }
    fn load(
        &mut self,
        _config: &Config,
        state: &mut State,
        _query: String,
        _item: String,
    ) -> super::LoadStream {
        let livegrep_result = state.last_load_resp.clone();
        Box::pin(async_stream::stream! {
            let items = match livegrep_result {
                Some(resp) => resp.items,
                None => vec![],
            };
            yield Ok(LoadResp::new_with_default_header(items))
        })
    }
    fn preview(
        &self,
        _config: &Config,
        _state: &mut State,
        _win: &PreviewWindow,
        item: String,
    ) -> BoxFuture<'static, Result<PreviewResp>> {
        async move { preview(item).await }.boxed()
    }
    fn fzf_bindings(&self) -> (fzf::Bindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [
                execute!(b, |_mode,_config,state,_query,item| {
                    let opts = OpenOpts::Neovim { tabedit: false };
                    open(state, item, opts).await
                })
            ],
            "ctrl-t" => [
                execute!(b, |_mode,_config,state,_query,item| {
                    let opts = OpenOpts::Neovim { tabedit: true };
                    open(state, item, opts).await
                })
            ],
            "ctrl-space" => [
                select_and_execute!{b, |_mode,_config,state,_query,item|
                    "neovim" => {
                        let opts = OpenOpts::Neovim { tabedit: false };
                        open(state, item, opts).await
                    },
                    "browse-github" => {
                        let opts = OpenOpts::BrowseGithub;
                        open(state, item, opts).await
                    },
                }
            ]
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Common
////////////////////////////////////////////////////////////////////////////////

static ITEM_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?P<file>[^:]*):(?P<line>\d+):(?P<col>\d+):.*").unwrap());

async fn preview(item: String) -> Result<PreviewResp> {
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
            let message = bat::render_file_with_highlight(&file, line).await?;
            Ok(PreviewResp { message })
        }
        Err(e) => {
            error!("rg: preview: parse line failed"; "error" => e.to_string(), "line" => line);
            Ok(PreviewResp {
                message: "".to_string(),
            })
        }
    }
}

enum OpenOpts {
    Neovim { tabedit: bool },
    BrowseGithub,
}

async fn open(state: &mut State, item: String, opts: OpenOpts) -> Result<()> {
    let file = ITEM_PATTERN.replace(&item, "$file").into_owned();
    let line = ITEM_PATTERN.replace(&item, "$line").into_owned();

    match opts {
        OpenOpts::Neovim { tabedit } => {
            let nvim = state.nvim.clone();
            let nvim_opts = nvim::OpenOpts {
                line: line.parse::<usize>().ok(),
                tabedit,
            };
            nvim.open(file.into(), nvim_opts).await?;
        }
        OpenOpts::BrowseGithub => {
            let revision = git::rev_parse("HEAD")?;
            gh::browse_github_line(file, &revision, line.parse::<usize>().unwrap()).await?;
        }
    }

    Ok(())
}
