use std::future::Future;

use anyhow::Result;
use futures::future::BoxFuture;
use futures::FutureExt;
use futures::StreamExt as _;
use tokio::process::Command;

use crate::config::Config;
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
use crate::utils::command::edit_and_run;
use crate::utils::fd;
use crate::utils::fzf;
use crate::utils::fzf::PreviewWindow;
use crate::utils::gh;
use crate::utils::vscode;
use crate::utils::xsel;

#[derive(Clone)]
pub struct Fd;

impl ModeDef for Fd {
    fn name(&self) -> &'static str {
        "fd"
    }
    fn load(
        &self,
        _config: &Config,
        _state: &mut State,
        _query: String,
        _item: String,
    ) -> super::LoadStream {
        load(fd::new())
    }
    fn preview(
        &self,
        _config: &Config,
        _win: &PreviewWindow,
        item: String,
    ) -> BoxFuture<'static, Result<PreviewResp>> {
        preview(item, |path: String| bat::render_file(path))
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
            "ctrl-space" => [
                execute!(b, |_mode,config,_state,_query,item| {
                    let opts = OpenOpts::VSCode;
                    open(config, item, opts).await
                })
            ],
            "ctrl-v" => [
                execute!(b, |_mode,config,_state,_query,item| {
                    let opts = OpenOpts::Vifm;
                    open(config, item, opts).await
                })
            ],
            "ctrl-y" => [
                execute!(b, |_mode,_config,_state,_query,item| {
                    xsel::yank(item).await?;
                    Ok(())
                })
            ],
            "pgup" => [
                select_and_execute!{b, |_mode,config,_state,_query,item|
                    "vscode" => {
                        let opts = OpenOpts::VSCode;
                        open(config, item, opts).await
                    },
                    "oil" => {
                        let cwd = std::env::current_dir().unwrap();
                        let opts = OpenOpts::Oil;
                        open(config, format!("{}", cwd.display()), opts).await
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
                    "browse-github" => {
                        let opts = OpenOpts::BrowseGithub;
                        open(config, item, opts).await
                    },
                    "xdragon" => {
                        let opts = OpenOpts::Xdragon;
                        open(config, item, opts).await
                    },
                }
            ]
        }
    }
}

enum OpenOpts {
    Neovim { tabedit: bool },
    VSCode,
    Oil,
    Vifm,
    BrowseGithub,
    Xdragon,
}

async fn open(config: &Config, file: String, opts: OpenOpts) -> Result<()> {
    match opts {
        OpenOpts::Neovim { tabedit } => {
            let nvim = config.nvim.clone();
            let nvim_opts = nvim::OpenOpts {
                line: None,
                tabedit,
            };
            nvim.open(file.into(), nvim_opts).await?
        }
        OpenOpts::VSCode => {
            let output = vscode::open(file, None).await?;
            config.nvim.notify_command_result("code", output).await?;
        }
        OpenOpts::Vifm => {
            let pwd = std::env::current_dir().unwrap().into_os_string();
            Command::new("vifm").arg(&pwd).spawn()?.wait().await?;
        }
        OpenOpts::Oil => {
            config.nvim.hide_floaterm().await?;
            config
                .nvim
                .command(&format!("Oil --float {}", file))
                .await?;
        }
        OpenOpts::BrowseGithub => {
            gh::browse_github(file).await?;
        }
        OpenOpts::Xdragon => {
            Command::new("xdragon").arg(&file).spawn()?.wait().await?;
        }
    }
    Ok(())
}

pub fn load(command: Command) -> super::LoadStream<'static> {
    Box::pin(async_stream::stream! {
        let stream = command::command_output_stream(command).chunks(100);
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

pub fn preview<S, R, Fut>(item: S, renderer: R) -> BoxFuture<'static, Result<PreviewResp>>
where
    S: AsRef<str> + Send + 'static,
    R: Fn(S) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<String>> + Send + 'static,
{
    async move {
        let message = renderer(item).await?;
        Ok(PreviewResp { message })
    }
    .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    #[tokio::test]
    async fn load_stream_yields_items_and_last() {
        // create a small shell script that outputs two lines
        let tmp = TempDir::new().unwrap();
        let script_path = tmp.path().join("fake_fd.sh");
        fs::write(&script_path, b"#!/usr/bin/env bash\necho a\necho b\n").unwrap();
        let mut perms = fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms).unwrap();

        let cmd = Command::new(&script_path);
        let mut stream = load(cmd).boxed();

        // first chunk
        let first = stream.next().await.expect("has first resp").unwrap();
        assert!(!first.is_last);
        assert_eq!(first.items, vec!["a".to_string(), "b".to_string()]);

        // final
        let last = stream.next().await.expect("has last resp").unwrap();
        assert!(last.is_last);

        // no more
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn preview_impl_uses_renderer() {
        let f = |_: String| async move { Ok::<_, anyhow::Error>("BAT OK".to_string()) };
        let resp = preview("/tmp/dummy".to_string(), f).await.unwrap();
        assert!(resp.message.contains("BAT OK"));
    }
}
