pub mod lib;

pub mod bookmark;
pub mod browser_bookmark;
pub mod browser_history;
pub mod buffer;
pub mod diagnostics;
pub mod fd;
pub mod git_branch;
pub mod git_diff;
pub mod git_log;
pub mod git_reflog;
pub mod git_review;
pub mod git_status;
pub mod livegrep;
pub mod mark;
pub mod menu;
pub mod mru;
pub mod nvim_session;
pub mod pr;
pub mod process_compose;
pub mod runner;
pub mod visits;
pub mod zoxide;

use anyhow::Result;
use futures::future::BoxFuture;
use futures::FutureExt;
use futures::Stream;
use std::pin::Pin;

use crate::env::Env;
use crate::method::LoadResp;
use crate::method::PreviewResp;
use crate::state::State;
use crate::utils::fzf;
use crate::utils::fzf::PreviewWindow;

pub trait AsAny: 'static {
    fn as_any(&self) -> &dyn std::any::Any;
}
impl<T: 'static> AsAny for T {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub type MkMode = Pin<Box<dyn (Fn() -> Mode) + Send + Sync>>;

pub fn all_modes() -> Vec<(String, MkMode)> {
    fn f(mode_def: impl ModeDef + Sync + Send + 'static) -> Mode {
        Mode {
            mode_def: Box::new(mode_def),
        }
    }
    let (runner, runner_commands) = runner::new_modes();
    let modes: Vec<MkMode> = vec![
        Box::pin(|| f(menu::Menu)),
        Box::pin(|| f(fd::Fd)),
        Box::pin(|| f(buffer::Buffer)),
        Box::pin(|| f(bookmark::Bookmark::new())),
        Box::pin(|| f(mark::Mark::new())),
        Box::pin(|| f(zoxide::Zoxide)),
        Box::pin(|| f(mru::Mru)),
        Box::pin(|| f(diagnostics::Diagnostics::new())),
        Box::pin(|| f(browser_history::BrowserHistory::new())),
        Box::pin(|| f(browser_bookmark::BrowserBookmark::new())),
        Box::pin(|| f(git_branch::GitBranch)),
        Box::pin(|| f(git_log::GitLog::Head)),
        Box::pin(|| f(git_log::GitLog::All)),
        Box::pin(|| f(git_reflog::GitReflog)),
        Box::pin(|| f(git_review::GitReview::new())),
        Box::pin(|| f(git_status::GitStatus)),
        Box::pin(|| f(git_diff::GitDiff::new())),
        Box::pin(|| f(nvim_session::NeovimSession)),
        Box::pin(|| f(livegrep::LiveGrep::new())),
        Box::pin(|| f(livegrep::LiveGrep::new_no_ignore())),
        Box::pin(|| f(livegrep::LiveGrepF)),
        Box::pin(|| f(visits::Visits::all())),
        Box::pin(|| f(visits::Visits::project())),
        Box::pin(|| f(pr::GhPr::Open)),
        Box::pin(|| f(pr::GhPr::All)),
        Box::pin(|| f(process_compose::ProcessCompose::new())),
        Box::pin(move || f(runner.clone())),
        Box::pin(move || f(runner_commands.clone())),
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
}

pub type LoadStream<'a> = Pin<Box<dyn Stream<Item = Result<LoadResp>> + Send + 'a>>;

pub trait ModeDef: AsAny {
    /// The name of the mode
    fn name(&self) -> &'static str;

    fn fzf_prompt(&self) -> String {
        format!("{}>", self.name())
    }

    fn fzf_bindings(&self) -> (fzf::Bindings, CallbackMap) {
        config_builder::default_bindings()
    }

    /// モード切替時に fzf に送る追加アクション (reload/change-prompt 以外)
    fn mode_enter_actions(&self) -> Vec<fzf::Action> {
        vec![]
    }

    /// このモードで sort を有効にするか
    fn wants_sort(&self) -> bool {
        true
    }

    /// Load items into fzf
    fn load<'a>(
        &'a self,
        config: &'a Env,
        state: &'a mut State,
        query: String,
        item: String, // currently selected item
    ) -> LoadStream<'a>;

    /// Preview the currently selected item
    fn preview<'a>(
        &'a self,
        config: &'a Env,
        win: &'a PreviewWindow,
        item: String,
    ) -> BoxFuture<'a, Result<PreviewResp>>;

    /// Execute the currently selected item
    /// (Optional. Intended to be used by the callback of fzf_bindings)
    fn execute<'a>(
        &'a self,
        _config: &'a Env,
        _state: &'a mut State,
        _item: String,
        _args: serde_json::Value,
    ) -> BoxFuture<'a, Result<()>> {
        async move { Ok(()) }.boxed()
    }
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
        dyn for<'a> Fn(
                &'a (dyn ModeDef + Sync + Send),
                &'a Env,
                &'a mut State,
                String,
                String,
            ) -> LoadStream<'a>
            + Sync
            + Send,
    >,
}

#[allow(clippy::type_complexity)]
pub struct PreviewCallback {
    pub callback: Box<
        dyn for<'a> Fn(
                &'a (dyn ModeDef + Sync + Send),
                &'a Env,
                &'a PreviewWindow,
                String,
            ) -> BoxFuture<'a, Result<PreviewResp>>
            + Sync
            + Send,
    >,
}

#[allow(clippy::type_complexity)]
pub struct ExecuteCallback {
    pub callback: Box<
        dyn for<'a> Fn(
                &'a (dyn ModeDef + Sync + Send),
                &'a Env,
                &'a mut State,
                String,
                String,
            ) -> BoxFuture<'a, Result<()>>
            + Sync
            + Send,
    >,
}

/// モード切替の共通処理: state 更新 + fzf アクション生成 + POST
pub async fn do_change_mode(
    env: &Env,
    state: &mut State,
    mode_name: &str,
    keep_query: bool,
) -> anyhow::Result<()> {
    let mode_info = env
        .mode_infos
        .get(mode_name)
        .ok_or_else(|| anyhow::anyhow!("unknown mode: {}", mode_name))?;

    state.set_current_mode_name(mode_name.to_string());

    let mut actions: Vec<fzf::Action> = vec![
        fzf::Action::Reload("load default '' ''".to_string()),
        fzf::Action::ChangePrompt(mode_info.prompt.clone()),
    ];

    if !keep_query {
        actions.push(fzf::Action::ClearQuery);
    }

    if state.sort_enabled() != mode_info.wants_sort {
        actions.push(fzf::Action::ToggleSort);
    }
    state.set_sort_enabled(mode_info.wants_sort);

    if mode_info.disable_search {
        actions.push(fzf::Action::DisableSearch);
    } else {
        actions.push(fzf::Action::EnableSearch);
    }

    actions.push(fzf::Action::ChangePreviewWindow(
        mode_info
            .custom_preview_window
            .clone()
            .unwrap_or_else(|| "right:50%:noborder".to_string()),
    ));

    actions.push(fzf::Action::DeselectAll);

    env.fzf_client.post_actions(&actions).await
}

pub mod config_builder {
    #![allow(dead_code)]
    use crate::env::Env;
    use crate::mode::lib::actions;
    use crate::mode::lib::item::ItemExtractor;
    use crate::mode::ModeDef;
    use crate::nvim::NeovimExt;
    use crate::state::State;
    use crate::utils::fzf;
    use anyhow::Result;
    use futures::future::BoxFuture;
    use futures::FutureExt;

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
            for<'a> F: Fn(
                    &'a (dyn ModeDef + Sync + Send),
                    &'a Env,
                    &'a mut State,
                    String,
                    String,
                ) -> BoxFuture<'a, Result<()>>
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
            for<'a> F: Fn(
                    &'a (dyn ModeDef + Sync + Send),
                    &'a Env,
                    &'a mut State,
                    String,
                    String,
                ) -> BoxFuture<'a, Result<()>>
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
            for<'a> F: Fn(
                    &'a (dyn ModeDef + Sync + Send),
                    &'a Env,
                    &'a mut State,
                    String,
                    String,
                ) -> super::LoadStream<'a>
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

        pub fn change_mode(&mut self, mode: impl Into<String>, keep_query: bool) -> fzf::Action {
            let mode_name = mode.into();
            self.execute_silent(move |_mode_def, env, state, _query, _item| {
                let mode_name = mode_name.clone();
                async move { super::do_change_mode(env, state, &mode_name, keep_query).await }
                    .boxed()
            })
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

        pub fn execute_as<M, F>(&mut self, callback: F) -> fzf::Action
        where
            M: ModeDef + Send + Sync + 'static,
            F: for<'a> Fn(
                    &'a M,
                    &'a Env,
                    &'a mut State,
                    String,
                    String,
                ) -> BoxFuture<'a, Result<()>>
                + Send
                + Sync
                + 'static,
        {
            self.execute(move |mode_dyn, config, state, query, item| {
                let mode = mode_dyn
                    .as_any()
                    .downcast_ref::<M>()
                    .expect("ConfigBuilder type mismatch: callback registered for wrong mode");
                callback(mode, config, state, query, item)
            })
        }

        pub fn execute_silent_as<M, F>(&mut self, callback: F) -> fzf::Action
        where
            M: ModeDef + Send + Sync + 'static,
            F: for<'a> Fn(
                    &'a M,
                    &'a Env,
                    &'a mut State,
                    String,
                    String,
                ) -> BoxFuture<'a, Result<()>>
                + Send
                + Sync
                + 'static,
        {
            self.execute_silent(move |mode_dyn, config, state, query, item| {
                let mode = mode_dyn
                    .as_any()
                    .downcast_ref::<M>()
                    .expect("ConfigBuilder type mismatch: callback registered for wrong mode");
                callback(mode, config, state, query, item)
            })
        }

        pub fn reload_with_as<M, F>(&mut self, callback: F) -> fzf::Action
        where
            M: ModeDef + Send + Sync + 'static,
            for<'a> F: Fn(&'a M, &'a Env, &'a mut State, String, String) -> super::LoadStream<'a>
                + Send
                + Sync
                + 'static,
        {
            self.reload_with(move |mode_dyn, config, state, query, item| {
                let mode = mode_dyn
                    .as_any()
                    .downcast_ref::<M>()
                    .expect("ConfigBuilder type mismatch: callback registered for wrong mode");
                callback(mode, config, state, query, item)
            })
        }

        fn gen_name(&mut self) -> String {
            self.callback_counter += 1;
            format!("callback{}", self.callback_counter)
        }

        /// Neovim でファイルを開く execute アクションを生成
        pub fn open_nvim<E: ItemExtractor>(&mut self, extractor: E, tabedit: bool) -> fzf::Action {
            self.execute(move |_mode, config, _state, _query, item| {
                let e = extractor.clone();
                async move {
                    let file = e.file(&item)?;
                    let line = e.line(&item);
                    actions::open_in_nvim(config, file, line, tabedit).await
                }
                .boxed()
            })
        }

        /// Neovim でファイルを開く execute_silent アクションを生成
        pub fn open_nvim_silent<E: ItemExtractor>(
            &mut self,
            extractor: E,
            tabedit: bool,
        ) -> fzf::Action {
            self.execute_silent(move |_mode, config, _state, _query, item| {
                let e = extractor.clone();
                async move {
                    let file = e.file(&item)?;
                    let line = e.line(&item);
                    actions::open_in_nvim(config, file, line, tabedit).await
                }
                .boxed()
            })
        }

        /// VSCode でファイルを開く execute アクションを生成
        pub fn open_vscode<E: ItemExtractor>(&mut self, extractor: E) -> fzf::Action {
            self.execute(move |_mode, config, _state, _query, item| {
                let e = extractor.clone();
                async move {
                    let file = e.file(&item)?;
                    let line = e.line(&item);
                    actions::open_in_vscode(config, file, line).await
                }
                .boxed()
            })
        }

        /// ファイルパスを yank する execute_silent アクションを生成
        pub fn yank_file<E: ItemExtractor>(&mut self, extractor: E) -> fzf::Action {
            self.execute_silent(move |_mode, _config, _state, _query, item| {
                let e = extractor.clone();
                async move { actions::yank(e.file(&item)?).await }.boxed()
            })
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
                $crate::utils::fzf::Bindings(core::convert::From::from([$(
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
            $builder.execute_as::<Self, _>(|$mode, $config, $state, $query, $item| {
                async move { $v }.boxed()
            })
        };
    }
    pub use execute;

    #[macro_export]
    macro_rules! execute_silent {
        ($builder:ident, |$mode:ident, $config:ident, $state:ident, $query:ident, $item:ident| $v:expr) => {
            $builder.execute_silent_as::<Self, _>(|$mode, $config, $state, $query, $item| {
                async move { $v }.boxed()
            })
        };
    }
    pub use execute_silent;

    #[macro_export]
    macro_rules! select_and_execute {
        ($builder:ident, |$mode:ident, $config:ident, $state:ident, $query:ident, $item:ident|
         $($k:expr => $v:expr),* $(,)?) => {
            $builder.execute_as::<Self, _>(|$mode, $config, $state, $query, $item| async move {
                match &*$crate::utils::fzf::select(vec![$($k),*]).await? {
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
                b.raw("change-preview-window[bottom:90%:border-top|right:50%:noborder]"),
            ],
            "pgdn" => [
                b.change_mode(super::menu::Menu.name(), false),
            ],
            "ctrl-f" => [
                b.change_mode(super::fd::Fd.name(), false),
            ],
            "ctrl-h" => [
                b.change_mode(super::visits::Visits::project().name(), false),
            ],
            "ctrl-d" => [
                b.change_mode(super::bookmark::Bookmark.name(), false),
            ],
            "ctrl-b" => [
                b.change_mode(super::buffer::Buffer.name(), false),
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
                b.change_mode(super::diagnostics::Diagnostics::new().name(), false),
            ],
            "alt-h" => [
                b.change_mode(super::browser_history::BrowserHistory::new().name(), false),
            ],
            "alt-b" => [
                b.change_mode(super::browser_bookmark::BrowserBookmark::new().name(), false),
            ],
            "ctrl-u" => [
                b.execute_silent(|_mode, _env, _state, _query, _item| {
                    async move {
                        let mut dir = std::env::current_dir()?;
                        dir.pop();
                        std::env::set_current_dir(dir)?;
                        Ok(())
                    }.boxed()
                }),
                b.reload(),
            ],
            "ctrl-l" => [
                b.execute_silent(|_mode, _env, _state, _query, item| {
                    async move {
                        let path = std::fs::canonicalize(&item)?;
                        let dir = match std::fs::metadata(&path) {
                            Ok(m) if m.is_file() => path.parent()
                                .ok_or_else(|| anyhow::anyhow!("no parent dir"))?
                                .to_owned(),
                            _ => path,
                        };
                        std::env::set_current_dir(dir)?;
                        Ok(())
                    }.boxed()
                }),
                b.clear_query(),
                b.reload(),
            ],
            "ctrl-n" => [
                b.execute_silent(|_mode, env, _state, _query, _item| {
                    async move {
                        let path = env.nvim.last_opened_file().await?;
                        let path = std::fs::canonicalize(path)?;
                        let dir = path.parent()
                            .ok_or_else(|| anyhow::anyhow!("no parent dir"))?;
                        std::env::set_current_dir(dir)?;
                        Ok(())
                    }.boxed()
                }),
                b.reload(),
            ],
        }
    }
}
