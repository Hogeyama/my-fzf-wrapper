pub mod bookmark;
pub mod browser_history;
pub mod buffer;
pub mod diagnostics;
pub mod fd;
pub mod git_branch;
pub mod git_diff;
pub mod git_log;
pub mod git_reflog;
pub mod git_status;
pub mod livegrep;
pub mod mark;
pub mod menu;
pub mod mru;
pub mod nvim_session;
pub mod zoxide;

use std::pin::Pin;

use crate::{
    config::Config,
    external_command::fzf,
    method::{LoadResp, PreviewResp},
    state::State,
};

use futures::{future::BoxFuture, FutureExt};

pub type MkMode = Pin<Box<dyn (Fn() -> Mode) + Send + Sync>>;

pub fn all_modes() -> Vec<(String, MkMode)> {
    fn f(mode_def: impl ModeDef + Sync + Send + 'static) -> Mode {
        Mode {
            mode_def: Box::new(mode_def),
        }
    }
    let modes: Vec<MkMode> = vec![
        Box::pin(|| f(menu::Menu)),
        Box::pin(|| f(fd::Fd)),
        Box::pin(|| f(buffer::Buffer)),
        Box::pin(|| f(bookmark::Bookmark::new())),
        Box::pin(|| f(mark::Mark::new())),
        Box::pin(|| f(zoxide::Zoxide)),
        Box::pin(|| f(mru::Mru)),
        Box::pin(|| f(diagnostics::Diagnostics)),
        Box::pin(|| f(browser_history::BrowserHistory)),
        Box::pin(|| f(git_branch::GitBranch)),
        Box::pin(|| f(git_log::GitLog::Head)),
        Box::pin(|| f(git_log::GitLog::All)),
        Box::pin(|| f(git_reflog::GitReflog)),
        Box::pin(|| f(git_status::GitStatus)),
        Box::pin(|| f(git_diff::GitDiff::new())),
        Box::pin(|| f(nvim_session::NeovimSession)),
        Box::pin(|| f(livegrep::LiveGrep::new())),
        Box::pin(|| f(livegrep::LiveGrep::new_no_ignore())),
        Box::pin(|| f(livegrep::LiveGrepF)),
    ];
    modes
        .into_iter()
        .map(|mk_mode| (mk_mode().name().to_string(), mk_mode))
        .collect()
}

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
                callback: Box::new(|mode_def, config, state, query, item| {
                    mode_def.load(config, state, query, item)
                }),
            },
        );
        callback_map.preview.insert(
            "default".to_string(),
            PreviewCallback {
                callback: Box::new(|mode_def, config, state, item| {
                    mode_def.preview(config, state, item)
                }),
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
            extra_opts: self
                .mode_def
                .fzf_extra_opts()
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
}

pub trait ModeDef {
    /// The name of the mode
    fn name(&self) -> &'static str;

    fn fzf_prompt(&self) -> String {
        format!("{}>", self.name())
    }

    fn fzf_bindings(&self) -> (fzf::Bindings, CallbackMap) {
        config_builder::default_bindings()
    }

    fn fzf_extra_opts(&self) -> Vec<&str> {
        vec![]
    }

    /// Load items into fzf
    fn load<'a>(
        &'a mut self,
        config: &'a Config,
        state: &'a mut State,
        query: String,
        item: String, // currently selected item
    ) -> BoxFuture<'a, Result<LoadResp, String>>;

    /// Preview the currently selected item
    fn preview<'a>(
        &'a self,
        config: &'a Config,
        state: &'a mut State,
        item: String,
    ) -> BoxFuture<'a, Result<PreviewResp, String>>;

    /// Execute the currently selected item
    /// (Optional. Intended to be used by the callback of fzf_bindings)
    fn execute<'a>(
        &'a mut self,
        _config: &'a Config,
        _state: &'a mut State,
        _item: String,
        _args: serde_json::Value,
    ) -> BoxFuture<'a, Result<(), String>> {
        async move { Ok(()) }.boxed()
    }
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

#[allow(clippy::type_complexity)]
pub struct LoadCallback {
    pub callback: Box<
        dyn for<'a> FnMut(
                &'a mut (dyn ModeDef + Sync + Send),
                &'a Config,
                &'a mut State,
                String,
                String,
            ) -> BoxFuture<'a, Result<LoadResp, String>>
            + Sync
            + Send,
    >,
}

#[allow(clippy::type_complexity)]
pub struct PreviewCallback {
    pub callback: Box<
        dyn for<'a> FnMut(
                &'a (dyn ModeDef + Sync + Send),
                &'a Config,
                &'a mut State,
                String,
            ) -> BoxFuture<'a, Result<PreviewResp, String>>
            + Sync
            + Send,
    >,
}

#[allow(clippy::type_complexity)]
pub struct ExecuteCallback {
    pub callback: Box<
        dyn for<'a> FnMut(
                &'a mut (dyn ModeDef + Sync + Send),
                &'a Config,
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
    use crate::{config::Config, external_command::fzf, mode::ModeDef, state::State};
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
                    &'a mut (dyn ModeDef + Sync + Send),
                    &'a Config,
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

        pub fn execute_silent<F>(&mut self, callback: F) -> fzf::Action
        where
            for<'a> F: FnMut(
                    &'a mut (dyn ModeDef + Sync + Send),
                    &'a Config,
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
            fzf::Action::ExecuteSilent(format!("execute {name} {{q}} {{}}"))
        }

        pub fn reload(&mut self) -> fzf::Action {
            self.reload_raw("load default {q} {}")
        }

        pub fn reload_with<F>(&mut self, callback: F) -> fzf::Action
        where
            for<'a> F: FnMut(
                    &'a mut (dyn ModeDef + Sync + Send),
                    &'a Config,
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

        pub fn execute_silent_raw(&self, cmd: impl Into<String>) -> fzf::Action {
            fzf::Action::ExecuteSilent(cmd.into())
        }

        pub fn execute_raw(&self, cmd: impl Into<String>) -> fzf::Action {
            fzf::Action::Execute(cmd.into())
        }

        pub fn change_mode(&self, mode: impl Into<String>, keep_query: bool) -> fzf::Action {
            fzf::Action::ExecuteSilent(format!(
                "change-mode {} {}",
                mode.into(),
                if keep_query { "{q}" } else { "" }, // query
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

        pub fn toggle(&self) -> fzf::Action {
            fzf::Action::Toggle
        }

        pub fn raw(&self, cmd: impl Into<String>) -> fzf::Action {
            fzf::Action::Raw(cmd.into())
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
            let mut $builder = $crate::mode::config_builder::ConfigBuilder::new();
            $builder.callback_counter = callback_map.execute.len() + callback_map.load.len();
            $builder.callback_map = callback_map;
            let bindings = bindings.merge(
                $crate::external_command::fzf::Bindings(core::convert::From::from([$(
                    ($k.to_string(), vec![$($v),*]),
                )*]))
            );
            (bindings, $builder.callback_map)
        }};
    }
    pub use bindings;

    #[macro_export]
    macro_rules! execute {
        ($builder:ident, |$mode:ident, $config:ident, $state:ident, $query:ident, $item:ident| $v:expr) => {
            $builder.execute(|$mode, $config, $state, $query, $item| async move { $v }.boxed())
        };
    }
    pub use execute;

    #[macro_export]
    macro_rules! execute_silent {
        ($builder:ident, |$mode:ident, $config:ident, $state:ident, $query:ident, $item:ident| $v:expr) => {
            $builder
                .execute_silent(|$mode, $config, $state, $query, $item| async move { $v }.boxed())
        };
    }
    pub use execute_silent;

    #[macro_export]
    macro_rules! select_and_execute {
        ($builder:ident, |$mode:ident, $config:ident, $state:ident, $query:ident, $item:ident|
         $($k:expr => $v:expr),* $(,)?) => {
            $builder.execute(|$mode, $config, $state, $query, $item| async move {
                match &*$crate::external_command::fzf::select(vec![$($k),*]).await? {
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
            "ctrl-r" => [
                b.reload(),
                b.clear_screen(),
            ],
            "shift-right" => [
                b.raw("change-preview-window[right:50%:noborder|right:80%:noborder]"),
            ],
            "pgdn" => [
                b.change_mode(super::menu::Menu.name(), false),
            ],
            "ctrl-f" => [
                b.change_mode(super::fd::Fd.name(), false),
            ],
            "ctrl-b" => [
                b.change_mode(super::buffer::Buffer.name(), false),
            ],
            "ctrl-d" => [
                b.change_mode(super::bookmark::Bookmark.name(), false),
            ],
            "ctrl-h" => [
                b.change_mode(super::mru::Mru.name(), false),
            ],
            "ctrl-j" => [
                b.change_mode(super::git_diff::GitDiff::new().name(), false),
            ],
            "ctrl-k" => [
                b.change_mode(super::git_branch::GitBranch.name(), false),
            ],
            "ctrl-o" => [
                b.change_mode(super::git_log::GitLog::Head.name(), false),
            ],
            "ctrl-g" => [
                b.change_mode(super::livegrep::LiveGrep::new().name(), true),
            ],
            "alt-d" => [
                b.change_mode(super::zoxide::Zoxide.name(), false),
            ],
            "alt-w" => [
                b.change_mode(super::diagnostics::Diagnostics.name(), false),
            ],
            "ctrl-u" => [
                b.execute_silent_raw("change-directory --to-parent"),
                b.reload(),
            ],
            "ctrl-l" => [
                b.execute_silent_raw("change-directory --dir {}"),
                b.clear_query(),
                b.reload(),
            ],
            "ctrl-n" => [
                b.execute_silent_raw("change-directory --to-last-file-dir"),
                b.reload(),
            ],
        }
    }
}
