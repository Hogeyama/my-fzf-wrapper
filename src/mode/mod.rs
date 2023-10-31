pub mod browser_history;
pub mod buffer;
pub mod diagnostics;
pub mod fd;
pub mod git_branch;
pub mod git_log;
pub mod git_reflog;
pub mod git_status;
pub mod livegrep;
pub mod menu;
pub mod mru;
pub mod nvim_session;
pub mod zoxide;

use crate::{
    external_command::fzf,
    method::{LoadResp, PreviewResp},
    state::State,
};

use futures::future::BoxFuture;

pub struct Mode {
    pub mode_def: Box<dyn ModeDef + Sync + Send>,
}

impl Mode {
    pub fn name(&self) -> &'static str {
        self.mode_def.name()
    }

    pub fn callbacks(&self) -> CallbackMap {
        let mut callback_map = self.mode_def.fzf_bindings().1;
        callback_map.load.insert(
            "default".to_string(),
            LoadCallback {
                callback: Box::new(|mode_def, state, query, item| {
                    mode_def.load(state, query, item)
                }),
            },
        );
        callback_map.preview.insert(
            "default".to_string(),
            PreviewCallback {
                callback: Box::new(|mode_def, state, item| mode_def.preview(state, item)),
            },
        );
        callback_map
    }

    pub fn fzf_config(&self, args: FzfArgs) -> fzf::Config {
        let bindings = self.mode_def.fzf_bindings().0;
        fzf::Config {
            myself: args.myself,
            socket: args.socket,
            log_file: args.log_file,
            load: vec![
                "load",
                "default",
                &args.initial_query.clone(),
                "", // item
            ]
            .into_iter()
            .map(|s| s.to_string())
            .collect(),
            initial_prompt: self.mode_def.fzf_prompt(),
            initial_query: args.initial_query,
            bindings,
            extra_opts: self.mode_def.fzf_extra_opts(),
        }
    }
}

pub trait ModeDef {
    /// The name of the mode
    fn name(&self) -> &'static str;

    fn new() -> Self
    where
        Self: Sized;

    fn fzf_prompt(&self) -> String {
        format!("{}>", self.name())
    }

    fn fzf_bindings<'a>(&'a self) -> (fzf::Bindings, CallbackMap) {
        config_builder::default_bindings()
    }

    fn fzf_extra_opts(&self) -> Vec<String> {
        vec![]
    }

    /// Load items into fzf
    fn load<'a>(
        &'a mut self,
        state: &'a mut State,
        query: String,
        item: String, // currently selected item
    ) -> BoxFuture<'a, Result<LoadResp, String>>;

    /// Run command with the selected item
    fn preview<'a>(
        &'a self,
        state: &'a mut State,
        item: String,
    ) -> BoxFuture<'a, Result<PreviewResp, String>>;
}

pub struct FzfArgs {
    pub myself: String,
    pub initial_query: String,
    pub socket: String,
    pub log_file: String,
}

pub struct CallbackMap {
    pub load: std::collections::HashMap<String, LoadCallback>,
    pub preview: std::collections::HashMap<String, PreviewCallback>,
    pub execute: std::collections::HashMap<String, ExecuteCallback>,
}
impl CallbackMap {
    pub fn empty() -> Self {
        CallbackMap {
            load: std::collections::HashMap::new(),
            preview: std::collections::HashMap::new(),
            execute: std::collections::HashMap::new(),
        }
    }
}

pub struct LoadCallback {
    pub callback: Box<
        dyn for<'a> FnMut(
                &'a mut (dyn ModeDef + Sync + Send),
                &'a mut State,
                String,
                String,
            ) -> BoxFuture<'a, Result<LoadResp, String>>
            + Sync
            + Send,
    >,
}

pub struct PreviewCallback {
    pub callback: Box<
        dyn for<'a> FnMut(
                &'a (dyn ModeDef + Sync + Send),
                &'a mut State,
                String,
            ) -> BoxFuture<'a, Result<PreviewResp, String>>
            + Sync
            + Send,
    >,
}

pub struct ExecuteCallback {
    pub callback: Box<
        dyn for<'a> FnMut(
                &'a (dyn ModeDef + Sync + Send),
                &'a mut State,
                String,
                String,
            ) -> BoxFuture<'a, Result<(), String>>
            + Sync
            + Send,
    >,
}

pub mod config_builder {
    #![allow(dead_code)]
    use crate::{external_command::fzf, mode::ModeDef, state::State};
    use futures::future::BoxFuture;

    pub struct ConfigBuilder {
        pub callback_map: super::CallbackMap,
        pub callback_counter: usize,
    }

    impl ConfigBuilder {
        pub fn new() -> Self {
            ConfigBuilder {
                callback_map: super::CallbackMap::empty(),
                callback_counter: 0,
            }
        }

        pub fn execute<F>(&mut self, callback: F) -> fzf::Action
        where
            for<'a> F: FnMut(
                    &'a (dyn ModeDef + Sync + Send),
                    &'a mut State,
                    String,
                    String,
                ) -> BoxFuture<'a, Result<(), String>>
                + Send
                + Sync
                + 'static,
        {
            let name = self.gen_name();
            let callback = Box::new(callback);
            self.callback_map
                .execute
                .insert(name.clone(), super::ExecuteCallback { callback });
            fzf::Action::Execute(format!("execute {name} {{q}} {{}}"))
        }

        pub fn reload(&mut self) -> fzf::Action {
            self.reload_raw("load default {q} {}")
        }

        pub fn reload_with<F>(&mut self, callback: F) -> fzf::Action
        where
            for<'a> F: FnMut(
                    &'a mut (dyn ModeDef + Sync + Send),
                    &'a mut State,
                    String,
                    String,
                ) -> BoxFuture<'a, Result<super::LoadResp, String>>
                + Send
                + Sync
                + 'static,
        {
            let name = self.gen_name();
            let callback = Box::new(callback);
            self.callback_map
                .load
                .insert(name.clone(), super::LoadCallback { callback });
            self.reload_raw(format!("load {name} {{q}} {{}}"))
        }

        pub fn reload_raw(&self, cmd: impl AsRef<str>) -> fzf::Action {
            fzf::Action::Reload(cmd.as_ref().to_string())
        }

        pub fn execute_raw(&self, cmd: impl Into<String>) -> fzf::Action {
            fzf::Action::Execute(cmd.into())
        }

        pub fn change_mode(&self, mode: impl Into<String>, keep_query: bool) -> fzf::Action {
            fzf::Action::Execute(format!(
                "change-mode {} {} {}",
                mode.into(),
                if keep_query { "{q}" } else { "" }, // query
                ""                                   // item
            ))
        }

        pub fn change_prompt(&self, prompt: impl Into<String>) -> fzf::Action {
            fzf::Action::ChangePrompt(prompt.into())
        }

        pub fn toggle_sort(&self) -> fzf::Action {
            fzf::Action::ToggleSort
        }

        pub fn clear_query(&self) -> fzf::Action {
            fzf::Action::ClearQuery
        }

        pub fn clear_screen(&self) -> fzf::Action {
            fzf::Action::ClearScreen
        }

        pub fn first(&self) -> fzf::Action {
            fzf::Action::First
        }

        fn gen_name(&mut self) -> String {
            self.callback_counter += 1;
            format!("callback{}", self.callback_counter)
        }
    }

    // TODO use gensym
    #[macro_export]
    macro_rules! bindings {
        ($builder:ident <= $init:expr,
         $($k:expr => [ $($v:expr),* $(,)? ] ),* $(,)?) => {{
            let (bindings, callback_map) = $init;
            let mut $builder = crate::mode::config_builder::ConfigBuilder::new();
            $builder.callback_counter = callback_map.execute.len() + callback_map.load.len();
            let bindings = bindings.merge(
                crate::external_command::fzf::Bindings(core::convert::From::from([$(
                    ($k.to_string(), vec![$($v),*]),
                )*]))
            );
            (bindings, $builder.callback_map)
        }};
    }
    pub use bindings;

    #[macro_export]
    macro_rules! execute {
        ($builder:ident, |$mode:ident, $state:ident, $query:ident, $item:ident| $v:expr) => {
            $builder.execute(|$mode, $state, $query, $item| async move { $v }.boxed())
        };
    }
    pub use execute;

    #[macro_export]
    macro_rules! select_and_execute {
        ($builder:ident, |$mode:ident, $state:ident, $query:ident, $item:ident|
         $($k:expr => $v:expr),* $(,)?) => {
            $builder.execute(|$mode, $state, $query, $item| async move {
                match &*crate::external_command::fzf::select(vec![$($k),*]).await? {
                    $($k => { $v })*
                    _ => { Ok(()) }
                }
            }.boxed())
        };
    }
    pub use select_and_execute;

    pub fn default_bindings() -> (fzf::Bindings, super::CallbackMap) {
        bindings! {
            b <= (fzf::Bindings::empty(), super::CallbackMap::empty()),
            "change" => [ b.first() ],
            "ctrl-s" => [ b.toggle_sort() ],
            "ctrl-o" => [ b.clear_query(), b.clear_screen() ],
            "ctrl-r" => [
                b.reload(),
                b.clear_screen(),
            ],
            "pgdn" => [
                b.change_mode(super::menu::Menu::new().name(), false),
            ],
            "ctrl-f" => [
                b.change_mode(super::fd::Fd::new().name(), false),
            ],
            "ctrl-b" => [
                b.change_mode(super::buffer::Buffer::new().name(), false),
            ],
            "ctrl-h" => [
                b.change_mode(super::mru::Mru::new().name(), false),
            ],
            "ctrl-g" => [
                b.change_mode(super::livegrep::LiveGrep::new().name(), true),
            ],
            "alt-d" => [
                b.change_mode(super::zoxide::Zoxide::new().name(), false),
            ],
            "alt-w" => [
                b.change_mode(super::diagnostics::Diagnostics::new().name(), false),
            ],
            "ctrl-i" => [
                b.change_mode(super::browser_history::BrowserHistory::new().name(), false),
            ],
            "ctrl-u" => [
                b.execute_raw("change-directory --to-parent"),
                b.reload(),
            ],
            "ctrl-l" => [
                b.execute_raw("change-directory --dir {}"),
                b.clear_query(),
                b.reload(),
            ],
            "ctrl-n" => [
                b.execute_raw("change-directory --to-last-file-dir"),
                b.reload(),
            ],
        }
    }
}
