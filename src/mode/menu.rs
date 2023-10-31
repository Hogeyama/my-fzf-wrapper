use crate::{
    config,
    external_command::fzf,
    method::{LoadResp, PreviewResp},
    mode::{config_builder, CallbackMap, ModeDef},
    state::State,
};

use futures::{future::BoxFuture, FutureExt};

#[derive(Clone)]
pub struct Menu;

impl ModeDef for Menu {
    fn new() -> Self {
        Menu
    }
    fn name(&self) -> &'static str {
        "menu"
    }
    fn load(
        &mut self,
        _state: &mut State,
        _query: String,
        _item: String,
    ) -> BoxFuture<'static, Result<LoadResp, String>> {
        async move {
            let items = config::new()
                .get_mode_names()
                .into_iter()
                .map(|s| s.to_string())
                .filter(|s| s != "rg" && s != "livegrep" && s != "livegrepf" && s != "menu") // FIXME ad-hoc
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
    fn fzf_bindings(&self) -> (fzf::Bindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [
                b.change_mode("{}", false),
            ],
        }
    }
}
