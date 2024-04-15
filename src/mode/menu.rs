use crate::{
    config::Config,
    method::{LoadResp, PreviewResp},
    mode::{config_builder, CallbackMap, ModeDef},
    state::State,
    utils::fzf::{self, PreviewWindow},
};

use futures::{future::BoxFuture, FutureExt};

#[derive(Clone)]
pub struct Menu;

impl ModeDef for Menu {
    fn name(&self) -> &'static str {
        "menu"
    }
    fn load<'a>(
        &'a mut self,
        config: &'a Config,
        _state: &'a mut State,
        _query: String,
        _item: String,
    ) -> BoxFuture<'a, Result<LoadResp, String>> {
        async move {
            let items = config
                .get_mode_names()
                .into_iter()
                .map(|s| s.to_string())
                .filter(|s| s != "livegrepf" && s != "menu") // FIXME ad-hoc
                .collect();
            Ok(LoadResp::new_with_default_header(items))
        }
        .boxed()
    }
    fn preview(
        &self,
        _config: &Config,
        _state: &mut State,
        _win: &PreviewWindow,
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
