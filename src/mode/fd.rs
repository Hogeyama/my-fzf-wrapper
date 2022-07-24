use std::error::Error;

use crate::{
    external_command::fd,
    method::{Load, LoadResp, Method, PreviewResp, RunResp},
    nvim::{focusing_last_win, last_opened_file, leaving_insert_mode, move_to_last_tab, Neovim},
    types::{Mode, State},
};

use clap::Parser;
use futures::{future::BoxFuture, FutureExt};
use tokio::process::Command as TokioCommand;

#[derive(Clone)]
pub struct Fd;

pub fn new() -> Fd {
    Fd
}

impl Mode for Fd {
    fn name(&self) -> &'static str {
        "fd"
    }
    fn load<'a>(
        &self,
        state: &'a mut State,
        opts: Vec<String>,
    ) -> BoxFuture<'a, <Load as Method>::Response> {
        let nvim = state.nvim.clone();
        async move {
            match clap_parse_from::<LoadOpts>(opts) {
                Ok(opts) => {
                    let () = change_directory(&nvim, opts).await;
                }
                Err(e) => {
                    error!("fd.run.opts failed"; "error" => e.to_string());
                }
            }
            let fd_result = fd::new().output().await;
            match fd_result {
                Ok(fd_output) => {
                    let pwd = std::env::current_dir().unwrap().into_os_string();
                    let fd_output = String::from_utf8_lossy(&fd_output.stdout)
                        .lines()
                        .map(|line| line.to_string())
                        .collect::<Vec<_>>();
                    LoadResp {
                        header: format!("[{}]", pwd.to_string_lossy()),
                        items: fd_output,
                    }
                }
                Err(fd_err) => LoadResp {
                    header: fd_err.to_string(),
                    items: vec![],
                },
            }
        }
        .boxed()
    }
    fn preview(&self, _state: &mut State, item: String) -> BoxFuture<'static, PreviewResp> {
        async move {
            let output = TokioCommand::new("bat")
                .arg(&item)
                .args(vec!["--color", "always"])
                .output()
                .await
                .map_err(|e| e.to_string())
                .expect("fd: preview:")
                .stdout;
            let output = String::from_utf8_lossy(output.as_slice()).into_owned();
            PreviewResp { message: output }
        }
        .boxed()
    }
    fn run<'a>(
        &self,
        state: &'a mut State,
        path: String,
        opts: Vec<String>,
    ) -> BoxFuture<'a, RunResp> {
        async move {
            let nvim = state.nvim.clone();
            match clap_parse_from::<RunOpts>(opts) {
                Ok(opts) => {
                    let _ = tokio::spawn(async move {
                        let r = nvim_open(&nvim, path.clone(), opts).await;
                        if let Err(e) = r {
                            error!("fd: run: nvim_open failed"; "error" => e.to_string());
                        }
                    });
                }
                Err(e) => {
                    error!("fd.run.opts failed"; "error" => e.to_string());
                }
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
    // Change directory to the given dir.
    // If a file is specified, change to the directory containing the file.
    #[clap(long)]
    cd: Option<String>,

    // Change directory to the parent directory.
    #[clap(long)]
    cd_up: bool,

    // Change directory to the parent directory.
    #[clap(long)]
    cd_last_file: bool,
}

// TODO これは util に置いた方がよいかもしれない
async fn change_directory(nvim: &Neovim, opts: LoadOpts) {
    fn cd_to(path: &str) {
        let path = std::fs::canonicalize(path).unwrap();
        match std::fs::metadata(&path) {
            // std::fs::metadata は symlink を follow してくれる
            Ok(metadata) => {
                if metadata.is_dir() {
                    std::env::set_current_dir(path).unwrap();
                } else {
                    let dir = path.parent().unwrap();
                    std::env::set_current_dir(dir).unwrap();
                }
            }
            Err(_) => {
                error!("fd: load: cd: directory not found"; "path" => path.into_os_string().into_string().unwrap());
            }
        }
    }
    info!("fd: load: opts"; "opts" => format!("{:?}", opts));
    match opts {
        LoadOpts { cd: Some(path), .. } => {
            cd_to(&path);
        }
        LoadOpts { cd_up, .. } if cd_up => {
            let mut dir = std::env::current_dir().unwrap();
            dir.pop();
            std::env::set_current_dir(dir).unwrap();
        }
        LoadOpts { cd_last_file, .. } if cd_last_file => {
            let last_file = last_opened_file(&nvim).await;
            match last_file {
                Ok(last_file) => {
                    cd_to(&last_file);
                }
                Err(e) => {
                    error!("fd: load: cd: last_file failed"; "error" => e.to_string());
                }
            }
        }
        _ => {}
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Run
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Parser, Debug, Clone)]
struct RunOpts {
    #[clap(long)]
    line: Option<u32>,

    #[clap(long)]
    tabedit: bool,
}

async fn nvim_open(nvim: &Neovim, path: String, opts: RunOpts) -> Result<(), Box<dyn Error>> {
    let line_opt = match opts.line {
        Some(line) => format!("+{line}"),
        None => "".to_string(),
    };
    let path = std::fs::canonicalize(path).unwrap();
    let path = path.to_string_lossy();

    if opts.tabedit {
        let cmd = format!("execute 'tabedit {line_opt} '.fnameescape('{path}')",);
        // open in new tab
        nvim.command(&cmd).await.map_err(|e| e.to_string())?;
        // return to fzf tab
        move_to_last_tab(&nvim).await?;
        Ok(())
    } else {
        leaving_insert_mode(&nvim, || {
            async {
                focusing_last_win(&nvim, || {
                    async {
                        let cmd = format!("execute 'edit {line_opt} '.fnameescape('{path}')");
                        nvim.command(&cmd).await.map_err(|e| e.to_string())?;
                        Ok(())
                    }
                    .boxed()
                })
                .await
            }
            .boxed()
        })
        .await
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Util
////////////////////////////////////////////////////////////////////////////////////////////////////

fn clap_parse_from<T: Parser>(args: Vec<String>) -> Result<T, clap::error::Error> {
    let mut clap_args = vec!["dummy".to_string()];
    clap_args.extend(args);
    T::try_parse_from(clap_args)
}
