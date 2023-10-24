use crate::{
    config,
    external_command::fzf,
    method::{LoadResp, PreviewResp, RunOpts, RunResp},
    types::{default_bindings, Mode, State},
};

use futures::{future::BoxFuture, FutureExt};

#[derive(Clone)]
pub struct Menu;

impl Mode for Menu {
    fn new() -> Self {
        Menu
    }
    fn name(&self) -> &'static str {
        "menu"
    }
    fn fzf_bindings(&self) -> fzf::Bindings {
        use fzf::*;
        default_bindings().merge(bindings! {
            "enter" => vec![
                execute("change-mode -- {}"),
            ]
        })
    }
    fn load(
        &self,
        _state: &mut State,
        _opts: Vec<String>,
    ) -> BoxFuture<'static, Result<LoadResp, String>> {
        async move {
            let items = config::new()
                .get_mode_names()
                .into_iter()
                .map(|s| s.to_string())
                .filter(|s| s != "rg" && s != "livegrep" && s != "menu") // FIXME ad-hoc
                .collect();
            Ok(LoadResp::new_with_default_header(items))
        }
        .boxed()
    }
    fn preview(
        &self,
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
        &self,
        _state: &mut State,
        _mode: String,
        _opts: RunOpts,
    ) -> BoxFuture<'static, Result<RunResp, String>> {
        panic!("unreachable")
    }
}
