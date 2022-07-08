use crate::{
    external_command::fd,
    logger,
    method::{Load, LoadResp, Method, PreviewResp, RunResp},
    types::State,
};

use futures::{future::BoxFuture, FutureExt};

use tokio::process::Command as TokioCommand;

use crate::types::Mode;

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
        _arg: Vec<String>,
    ) -> BoxFuture<'a, <Load as Method>::Response> {
        async {
            let fd_result = fd::new().output().await;
            match fd_result {
                Ok(fd_output) => {
                    let pwd = state.pwd.clone().into_os_string();
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
        item: String,
        _args: Vec<String>,
    ) -> BoxFuture<'a, RunResp> {
        async move {
            let nvim = state.nvim.clone();
            let _ = tokio::spawn(async move {
                let commands = vec![
                    "MoveToLastWin".to_string(),
                    format!("execute 'edit '.fnameescape('{item}')"),
                    "MoveToLastWin".to_string(),
                    "startinsert".to_string(),
                ];
                logger::info("fd.run.nvim_command", &commands);
                let capture_output = false;
                let _ = nvim.command("stopinsert").await; // 個別に実行する必要がある
                let r = nvim.exec(&commands.join("\n"), capture_output).await;
                if let Err(e) = r {
                    logger::error("fd.run.nvim_command", e.to_string());
                }
            });
            RunResp
        }
        .boxed()
    }
}
