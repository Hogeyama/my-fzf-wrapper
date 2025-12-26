use anyhow::Result;
use futures::future::BoxFuture;
use futures::FutureExt;

use crate::config::Config;
use crate::method::LoadResp;
use crate::method::PreviewResp;
use crate::mode::config_builder;
use crate::mode::CallbackMap;
use crate::mode::ModeDef;
use crate::state::State;
use crate::utils::fzf;
use crate::utils::fzf::PreviewWindow;

#[derive(Clone)]
pub struct Menu;

impl ModeDef for Menu {
    fn name(&self) -> &'static str {
        "menu"
    }
    fn load<'a>(
        &'a self,
        config: &'a Config,
        _state: &'a mut State,
        _query: String,
        _item: String,
    ) -> super::LoadStream<'a> {
        Box::pin(async_stream::stream! {
            let items = config
                .get_mode_names()
                .into_iter()
                .map(|s| s.to_string())
                .filter(|s| s != "menu"
                      && s != "livegrepf"
                      && s != "runner_commands"
                ) // FIXME ad-hoc
                .collect();
            yield Ok(LoadResp::new_with_default_header(items))
        })
    }
    fn preview(
        &self,
        _config: &Config,
        _win: &PreviewWindow,
        _item: String,
    ) -> BoxFuture<'static, Result<PreviewResp>> {
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
