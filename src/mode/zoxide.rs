use crate::{
    external_command::zoxide,
    method::{Load, LoadResp, Method, PreviewResp, RunOpts, RunResp},
    types::{Mode, State},
};

use futures::{future::BoxFuture, FutureExt};
use tokio::process::Command as TokioCommand;

#[derive(Clone)]
pub struct Zoxide;

pub fn new() -> Zoxide {
    Zoxide
}

impl Mode for Zoxide {
    fn name(&self) -> &'static str {
        "zoxide"
    }
    fn load<'a>(
        &mut self,
        _state: &'a mut State,
        _opts: Vec<String>,
    ) -> BoxFuture<'a, <Load as Method>::Response> {
        async move {
            let zoxide_result = zoxide::new().output().await;
            match zoxide_result {
                Ok(zoxide_output) => {
                    let pwd = std::env::current_dir().unwrap().into_os_string();
                    let zoxide_output = String::from_utf8_lossy(&zoxide_output.stdout)
                        .lines()
                        .map(|line| line.to_string())
                        .collect::<Vec<_>>();
                    LoadResp {
                        header: format!("[{}]", pwd.to_string_lossy()),
                        items: zoxide_output,
                    }
                }
                Err(zoxide_err) => {
                    error!("zoxide.run.opts failed"; "error" => zoxide_err.to_string());
                    LoadResp {
                        header: zoxide_err.to_string(),
                        items: vec![],
                    }
                }
            }
        }
        .boxed()
    }
    fn preview(&mut self, _state: &mut State, item: String) -> BoxFuture<'static, PreviewResp> {
        async move {
            let output = TokioCommand::new("exa")
                .args(vec!["--color", "always"])
                .args(vec!["--all"])
                .args(vec!["--sort", "name"])
                .args(vec!["--tree"])
                .args(vec!["--level", "1"])
                .args(vec!["--classify"])
                .args(vec!["--git"])
                .args(vec!["--color=always"])
                .arg(&item)
                .output()
                .await
                .map_err(|e| e.to_string())
                .expect("zoxide: preview:")
                .stdout;
            let output = String::from_utf8_lossy(output.as_slice()).into_owned();
            PreviewResp { message: output }
        }
        .boxed()
    }
    fn run<'a>(
        &mut self,
        _state: &'a mut State,
        _path: String,
        _opts: RunOpts,
    ) -> BoxFuture<'a, RunResp> {
        async move { RunResp }.boxed()
    }
}
