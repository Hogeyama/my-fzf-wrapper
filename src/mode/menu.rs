use crate::{
    config,
    method::{LoadResp, PreviewResp, RunOpts, RunResp},
    types::{Mode, State},
};

use futures::{future::BoxFuture, FutureExt};

#[derive(Clone)]
pub struct Menu;

pub fn new() -> Menu {
    Menu
}

impl Mode for Menu {
    fn name(&self) -> &'static str {
        "menu"
    }
    fn load(
        &mut self,
        _state: &mut State,
        _opts: Vec<String>,
    ) -> BoxFuture<'static, Result<LoadResp, String>> {
        async move {
            let items = config::new()
                .get_mode_names()
                .into_iter()
                .map(|s| s.to_string())
                .filter(|s| s != "rg" && s != "menu")
                .collect();
            Ok(LoadResp::new_with_default_header(items))
        }
        .boxed()
    }
    fn preview(
        &mut self,
        _state: &mut State,
        _item: String,
    ) -> BoxFuture<'static, Result<PreviewResp, String>> {
        async move {
            Ok(PreviewResp {
                message: "No description".to_string(),
            })
        }
        .boxed()
    }
    fn run(
        &mut self,
        state: &mut State,
        mode: String,
        _opts: RunOpts,
    ) -> BoxFuture<'static, Result<RunResp, String>> {
        let config = config::new();
        let mode = config.get_mode(&mode);
        state.mode = Some(mode);
        async move { Ok(RunResp) }.boxed()
    }
}
