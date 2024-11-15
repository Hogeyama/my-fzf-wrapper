use anyhow::Result;
use clap::Parser;
use futures::future::BoxFuture;
use futures::FutureExt;
use futures::StreamExt as _;
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
use crate::utils::command;
use crate::utils::fzf;
use crate::utils::fzf::PreviewWindow;
use crate::utils::gh;
use crate::utils::git;
use crate::utils::rg;
use crate::utils::vscode;

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
        &self,
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
            "esc" => [
                b.change_mode(LiveGrepF.name(), false),
            ],
            "enter" => [
                execute!(b, |_mode,config,_state,_query,item| {
                    let opts = if vscode::in_vscode() {
                        OpenOpts::VSCode
                    } else {
                        OpenOpts::Neovim { tabedit: false }
                    };
                    open(config, item, opts).await
                })
            ],
            "ctrl-t" => [
                execute!(b, |_mode,config,_state,_query,item| {
                    let opts = OpenOpts::Neovim { tabedit: true };
                    open(config, item, opts).await
                })
            ],
            "pgup" => [
                select_and_execute!{b, |_mode,config,_state,_query,item|
                    "neovim" => {
                        let opts = OpenOpts::Neovim { tabedit: false };
                        open(config, item, opts).await
                    },
                    "browse-github" => {
                        let opts = OpenOpts::BrowseGithub;
                        open(config, item, opts).await
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
        let stream = command::command_output_stream(rg_cmd).chunks(100); // tekito
        tokio::pin!(stream);
        let mut has_error = false;
        while let Some(r) = stream.next().await {
            let r = r.into_iter().collect::<Result<Vec<String>>>();
            match r {
                Ok(lines) => {
                    yield Ok(LoadResp::wip_with_default_header(lines));
                }
                Err(e) => {
                    yield Ok(LoadResp::error(e.to_string()));
                    has_error = true;
                    break;
                }
            }
        }
        if !has_error {
            yield Ok(LoadResp::last())
        }
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
        &self,
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
                execute!(b, |_mode,config,_state,_query,item| {
                    let opts = OpenOpts::Neovim { tabedit: false };
                    open(config, item, opts).await
                })
            ],
            "ctrl-t" => [
                execute!(b, |_mode,config,_state,_query,item| {
                    let opts = OpenOpts::Neovim { tabedit: true };
                    open(config, item, opts).await
                })
            ],
            "pgup" => [
                select_and_execute!{b, |_mode,config,_state,_query,item|
                    "neovim" => {
                        let opts = OpenOpts::Neovim { tabedit: false };
                        open(config, item, opts).await
                    },
                    "browse-github" => {
                        let opts = OpenOpts::BrowseGithub;
                        open(config, item, opts).await
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
    VSCode,
    BrowseGithub,
}

async fn open(config: &Config, item: String, opts: OpenOpts) -> Result<()> {
    let file = ITEM_PATTERN.replace(&item, "$file").into_owned();
    let line = ITEM_PATTERN.replace(&item, "$line").into_owned();

    match opts {
        OpenOpts::Neovim { tabedit } => {
            let nvim = config.nvim.clone();
            let nvim_opts = nvim::OpenOpts {
                line: line.parse::<usize>().ok(),
                tabedit,
            };
            nvim.open(file.into(), nvim_opts).await?;
        }
        OpenOpts::VSCode => {
            let line = line.parse::<usize>().ok();
            let output = vscode::open(file, line).await?;
            config.nvim.notify_command_result("code", output).await?;
        }
        OpenOpts::BrowseGithub => {
            let revision = git::rev_parse("HEAD")?;
            gh::browse_github_line(file, &revision, line.parse::<usize>().unwrap()).await?;
        }
    }

    Ok(())
}
