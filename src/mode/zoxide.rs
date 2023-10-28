use crate::state::State;
use crate::{
    external_command::zoxide,
    method::{LoadResp, PreviewResp},
    mode::ModeDef,
};

use futures::{future::BoxFuture, FutureExt};
use tokio::process::Command;

#[derive(Clone)]
pub struct Zoxide;

impl ModeDef for Zoxide {
    fn new() -> Self {
        Zoxide
    }
    fn name(&self) -> &'static str {
        "zoxide"
    }
    fn load(
        &self,
        _state: &mut State,
        _query: String,
        _item: String,
    ) -> BoxFuture<'static, Result<LoadResp, String>> {
        async move {
            let zoxide_output = zoxide::new().output().await.map_err(|e| e.to_string())?;
            let zoxide_output = String::from_utf8_lossy(&zoxide_output.stdout)
                .lines()
                .map(|line| line.to_string())
                .collect::<Vec<_>>();
            Ok(LoadResp::new_with_default_header(zoxide_output))
        }
        .boxed()
    }
    fn preview(
        &self,
        _state: &mut State,
        item: String,
    ) -> BoxFuture<'static, Result<PreviewResp, String>> {
        async move {
            let output = Command::new("exa")
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
            Ok(PreviewResp { message: output })
        }
        .boxed()
    }
}
