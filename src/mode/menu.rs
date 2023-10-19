use crate::{
    config,
    method::{Load, LoadResp, Method, PreviewResp, RunOpts, RunResp},
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
    ) -> BoxFuture<'static, <Load as Method>::Response> {
        async move {
            let pwd = std::env::current_dir().unwrap().into_os_string();
            let items = config::new()
                .get_mode_names()
                .into_iter()
                .map(|s| s.to_string())
                .filter(|s| s != "rg" && s != "menu")
                .collect();
            LoadResp {
                header: format!("[{}]", pwd.to_string_lossy()),
                items,
            }
        }
        .boxed()
    }
    fn preview(&mut self, _state: &mut State, _item: String) -> BoxFuture<'static, PreviewResp> {
        async move {
            PreviewResp {
                message: "No description".to_string(),
            }
        }
        .boxed()
    }
    fn run(
        &mut self,
        state: &mut State,
        mode: String,
        _opts: RunOpts,
    ) -> BoxFuture<'static, RunResp> {
        let config = config::new();
        let mode = config.get_mode(&mode);
        state.mode = Some(mode);
        async move { RunResp }.boxed()
    }
}
