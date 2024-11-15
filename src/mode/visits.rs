use anyhow::Result;
use futures::future::BoxFuture;
use futures::stream;
use futures::stream::StreamExt;
use futures::FutureExt;
use rmpv::ext::from_value;
use tokio::process::Command;

use crate::config::Config;
use crate::method::LoadResp;
use crate::method::PreviewResp;
use crate::mode::config_builder;
use crate::mode::CallbackMap;
use crate::mode::ModeDef;
use crate::nvim;
use crate::nvim::Neovim;
use crate::nvim::NeovimExt;
use crate::state::State;
use crate::utils::bat;
use crate::utils::command::edit_and_run;
use crate::utils::fzf;
use crate::utils::fzf::PreviewWindow;
use crate::utils::path::to_relpath;
use crate::utils::vscode;
use crate::utils::xsel;

#[derive(Clone)]
pub struct Visits {
    kind: VisitsKind,
}

#[derive(Clone, Copy)]
pub enum VisitsKind {
    All,
    Project,
}

impl Visits {
    pub fn new(kind: VisitsKind) -> Self {
        Self { kind }
    }
    pub fn all() -> Self {
        Self::new(VisitsKind::All)
    }
    pub fn project() -> Self {
        Self::new(VisitsKind::Project)
    }
}

impl ModeDef for Visits {
    fn name(&self) -> &'static str {
        match self.kind {
            VisitsKind::All => "visits:all",
            VisitsKind::Project => "visists:cwd",
        }
    }
    fn load(
        &self,
        config: &Config,
        _state: &mut State,
        _query: String,
        _item: String,
    ) -> super::LoadStream {
        let nvim = config.nvim.clone();
        let kind = self.kind;
        Box::pin(async_stream::stream! {
            let mru_items = get_visits(&nvim, kind).await?;
            yield Ok(LoadResp::new_with_default_header(mru_items))
        })
    }
    fn preview(
        &self,
        _config: &Config,
        _win: &PreviewWindow,
        item: String,
    ) -> BoxFuture<'static, Result<PreviewResp>> {
        async move {
            let meta = std::fs::metadata(&item);
            match meta {
                Ok(meta) if meta.is_file() => {
                    let message = bat::render_file(&item).await?;
                    Ok(PreviewResp { message })
                }
                _ => Ok(PreviewResp {
                    message: "No Preview".to_string(),
                }),
            }
        }
        .boxed()
    }
    fn fzf_bindings(&self) -> (fzf::Bindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
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
            "ctrl-y" => [
                execute!(b, |_mode,_config,_state,_query,item| {
                    xsel::yank(item).await?;
                    Ok(())
                })
            ],
            "ctrl-x" => [
                execute_silent!(b, |_mode,config,_state,_query,item| {
                    config.nvim.eval_lua(
                        format!("require'mini.visits'.remove_path('{}')", item)
                    ).await?;
                    Ok(())
                }),
                b.reload(),
            ],
            "pgup" => [
                select_and_execute!{b, |_mode,config,_state,_query,item|
                    "oil" => {
                        let cwd = std::env::current_dir().unwrap();
                        config.nvim.hide_floaterm().await?;
                        config
                            .nvim
                            .command(&format!("Oil --float {}", cwd.display()))
                            .await?;
                        Ok(())
                    },
                    "new file" => {
                        let cwd = std::env::current_dir().unwrap();
                        let fname = fzf::input_with_placeholder("Enter file name", &item).await?;
                        let fname = fname.trim();
                        let path = format!("{}/{}", cwd.display(), fname);
                        let dir = std::path::Path::new(&path).parent().unwrap();
                        Command::new("mkdir")
                            .arg("-p")
                            .arg(dir)
                            .status()
                            .await?;
                        Command::new("touch")
                            .arg(&path)
                            .status()
                            .await?;
                        let opts = if vscode::in_vscode() {
                            OpenOpts::VSCode
                        } else {
                            OpenOpts::Neovim { tabedit: false }
                        };
                        open(config, path, opts).await
                    },
                    "execute any command" => {
                        let (cmd, output) = edit_and_run(format!(" {item}"))
                            .await?;
                        config.nvim.notify_command_result(&cmd, output)
                            .await?;
                        Ok(())
                    },
                }
            ]
        }
    }
    fn fzf_extra_opts(&self) -> Vec<&str> {
        vec!["--no-sort"]
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Util
////////////////////////////////////////////////////////////////////////////////////////////////////

async fn is_file(path: String) -> bool {
    let meta = tokio::fs::metadata(path).await;
    matches!(meta, Ok(meta) if meta.is_file())
}

async fn get_visits(nvim: &Neovim, kind: VisitsKind) -> Result<Vec<String>> {
    let mrus: Vec<String> = from_value(
        nvim.eval_lua(format!(
            "return require'mini.visits'.list_paths({})",
            match kind {
                VisitsKind::All => "''",      // empty string for all project
                VisitsKind::Project => "nil", // nil for current project
            }
        ))
        .await?,
    )?;
    let mrus = stream::iter(mrus)
        .filter(|x| is_file(x.clone()))
        .map(to_relpath)
        .collect::<Vec<_>>()
        .await;
    Ok(mrus)
}

enum OpenOpts {
    Neovim { tabedit: bool },
    VSCode,
}

async fn open(config: &Config, item: String, opts: OpenOpts) -> Result<()> {
    match opts {
        OpenOpts::Neovim { tabedit } => {
            let nvim_opts = nvim::OpenOpts {
                line: None,
                tabedit,
            };
            config.nvim.open(item.into(), nvim_opts).await?;
        }
        OpenOpts::VSCode => {
            let output = vscode::open(item.into(), None).await?;
            config.nvim.notify_command_result("code", output).await?;
        }
    }
    Ok(())
}
