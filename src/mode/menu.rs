use anyhow::Result;
use futures::future::BoxFuture;
use futures::FutureExt;

use crate::env::Env;
use crate::method::LoadResp;
use crate::method::PreviewResp;
use crate::mode::config_builder;
use crate::mode::CallbackMap;
use crate::mode::ModeDef;
use crate::state::State;
use crate::utils::fzf::PreviewWindow;

#[derive(Clone)]
pub struct Menu;

impl ModeDef for Menu {
    fn name(&self) -> &'static str {
        "menu"
    }
    fn load<'a>(
        &'a self,
        env: &'a Env,
        _state: &'a State,
        _query: String,
        _item: String,
    ) -> super::LoadStream<'a> {
        Box::pin(async_stream::stream! {
            let items = env.config
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
        _env: &Env,
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
    fn fzf_bindings(&self) -> (super::ModeBindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [
                execute_silent!(b, |_mode, env, state, _query, item| {
                    super::do_change_mode(env, state, &item, false).await
                }),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_enter_binding_is_execute_callback() {
        let menu = Menu;
        let (bindings, callbacks) = menu.fzf_bindings();
        let config = crate::config::new(
            "fzfw".to_string(),
            "/tmp/test.sock".to_string(),
            "/tmp/test.log".to_string(),
        );
        let rendered = config.render_mode_bindings(&bindings);
        let enter = rendered.get("enter").unwrap();
        assert!(
            enter.contains("execute"),
            "expected execute callback, got: {}",
            enter
        );
        assert!(
            !callbacks.execute.is_empty(),
            "expected execute callbacks to be registered"
        );
    }

    #[test]
    fn build_all_modes_menu_enter() {
        let config = crate::config::new(
            "fzfw".to_string(),
            "/tmp/test.sock".to_string(),
            "/tmp/test.log".to_string(),
        );
        let all_modes = config.build_all_modes();
        let menu_entry = all_modes.get("menu").expect("menu mode not found");
        let enter = menu_entry
            .rendered_bindings
            .get("enter")
            .expect("enter binding not found");
        assert!(
            enter.contains("execute"),
            "menu's enter in all_modes should have execute callback, got: {}",
            enter
        );
    }
}
