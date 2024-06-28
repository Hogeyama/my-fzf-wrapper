use anyhow::Result;
use futures::future::BoxFuture;
use futures::FutureExt;
use tokio::process::Command;

use crate::config::Config;
use crate::method::LoadResp;
use crate::method::PreviewResp;
use crate::mode::ModeDef;
use crate::state::State;
use crate::utils::fzf::PreviewWindow;
use crate::utils::zoxide;

#[derive(Clone)]
pub struct Zoxide;

impl ModeDef for Zoxide {
    fn name(&self) -> &'static str {
        "zoxide"
    }
    fn load(
        &mut self,
        _config: &Config,
        _state: &mut State,
        _query: String,
        _item: String,
    ) -> super::LoadStream {
        Box::pin(async_stream::stream! {
            let zoxide_output = zoxide::new().output().await?;
            let zoxide_output = String::from_utf8_lossy(&zoxide_output.stdout)
                .lines()
                .map(|line| line.to_string())
                .collect::<Vec<_>>();
            yield Ok(LoadResp::new_with_default_header(zoxide_output))
        })
    }
    fn preview(
        &self,
        _config: &Config,
        _state: &mut State,
        _win: &PreviewWindow,
        item: String,
    ) -> BoxFuture<'static, Result<PreviewResp>> {
        async move {
            let output = Command::new("eza")
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
