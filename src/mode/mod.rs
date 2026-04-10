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
pub mod git_status;
pub mod livegrep;
pub mod mark;
pub mod menu;
pub mod mru;
pub mod nas_worktree;
pub mod pr_diff;
pub mod pr_list;
pub mod pr_threads;
pub mod runner;
pub mod visits;
pub mod zoxide;

use anyhow::Result;
use futures::future::BoxFuture;
use futures::FutureExt;
use futures::Stream;
use std::pin::Pin;

use std::collections::HashMap;

use crate::env::Env;
use crate::method::LoadResp;
use crate::method::PreviewResp;
use crate::utils::fzf;
use crate::utils::fzf::PreviewWindow;

// ---------------------------------------------------------------------------
// ModeAction / ModeBindings: モード実装で使うアクション型

#[derive(Clone)]
pub enum ModeAction {
    /// load コールバックを呼ぶ (reload)
    Reload(String),
    /// execute コールバックを呼ぶ (execute, fzf が終了を待つ)
    Execute(String),
    /// execute コールバックを呼ぶ (execute-silent, fzf が終了を待たない)
    ExecuteSilent(String),
    /// 純粋な fzf アクション
    Fzf(fzf::Action),
}

impl ModeAction {
    /// myself と CLI 引数構造を組み立てて fzf::Action に変換
    pub fn into_fzf_action(self, myself: &str) -> fzf::Action {
        match self {
            ModeAction::Reload(name) => {
                fzf::Action::Reload(format!("{myself} load {name} {{q}} {{}}"))
            }
            ModeAction::Execute(name) => {
                fzf::Action::Execute(format!("{myself} execute {name} {{q}} {{}} {{n}}"))
            }
            ModeAction::ExecuteSilent(name) => {
                fzf::Action::ExecuteSilent(format!("{myself} execute {name} {{q}} {{}} {{n}}"))
            }
            ModeAction::Fzf(a) => a,
        }
    }
}

pub struct ModeBindings(pub HashMap<String, Vec<ModeAction>>);

impl ModeBindings {
    pub fn empty() -> Self {
        ModeBindings(HashMap::new())
    }
    pub fn merge(mut self, other: Self) -> Self {
        self.0.extend(other.0);
        self
    }
    #[allow(dead_code)]
    pub fn remove_key(mut self, key: &str) -> Self {
        self.0.remove(key);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_action_reload_into_fzf_action() {
        let action = ModeAction::Reload("default".into());
        let rendered = action.into_fzf_action("fzfw").render();
        assert_eq!(rendered, "reload[fzfw load default {q} {}]");
    }

    #[test]
    fn mode_action_execute_into_fzf_action() {
        let action = ModeAction::Execute("cb0".into());
        let rendered = action.into_fzf_action("fzfw").render();
        assert_eq!(rendered, "execute[fzfw execute cb0 {q} {} {n}]");
    }

    #[test]
    fn mode_action_execute_silent_into_fzf_action() {
        let action = ModeAction::ExecuteSilent("cb1".into());
        let rendered = action.into_fzf_action("fzfw").render();
        assert_eq!(rendered, "execute-silent[fzfw execute cb1 {q} {} {n}]");
    }

    #[test]
    fn mode_action_fzf_passthrough() {
        let action = ModeAction::Fzf(fzf::Action::ToggleSort);
        let rendered = action.into_fzf_action("fzfw").render();
        assert_eq!(rendered, "toggle-sort");
    }

    #[test]
    fn mode_bindings_empty() {
        let bindings = ModeBindings::empty();
        assert!(bindings.0.is_empty());
    }

    #[test]
    fn mode_bindings_merge() {
        let a = ModeBindings(HashMap::from([(
            "enter".to_string(),
            vec![ModeAction::Fzf(fzf::Action::First)],
        )]));
        let b = ModeBindings(HashMap::from([(
            "ctrl-s".to_string(),
            vec![ModeAction::Fzf(fzf::Action::ToggleSort)],
        )]));
        let merged = a.merge(b);
        assert!(merged.0.contains_key("enter"));
        assert!(merged.0.contains_key("ctrl-s"));
    }

    #[test]
    fn mode_bindings_remove_key() {
        let bindings = ModeBindings(HashMap::from([(
            "enter".to_string(),
            vec![ModeAction::Fzf(fzf::Action::First)],
        )]));
        let bindings = bindings.remove_key("enter");
        assert!(!bindings.0.contains_key("enter"));
    }

    #[test]
    fn all_modes_has_unique_names() {
        let modes = all_modes();
        let mut seen = std::collections::HashSet::new();
        for (name, _) in &modes {
            assert!(seen.insert(name.clone()), "duplicate mode name: {}", name);
        }
    }

    #[test]
    fn all_modes_contains_menu() {
        let modes = all_modes();
        assert!(modes.iter().any(|(name, _)| name == "menu"));
    }

    #[test]
    fn all_modes_contains_fd() {
        let modes = all_modes();
        assert!(modes.iter().any(|(name, _)| name == "fd"));
    }

    #[test]
    fn default_bindings_has_ctrl_b_and_alt_u() {
        let (bindings, _callbacks) = config_builder::default_bindings();
        assert!(
            bindings.0.contains_key("ctrl-b"),
            "expected ctrl-b in default_bindings"
        );
        assert!(
            bindings.0.contains_key("alt-u"),
            "expected alt-u in default_bindings"
        );
    }
}

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
        Box::pin(|| f(pr_threads::GitReview::new())),
        Box::pin(|| f(git_status::GitStatus)),
        Box::pin(|| f(git_diff::GitDiff::new())),
        Box::pin(|| f(livegrep::LiveGrep::new())),
        Box::pin(|| f(livegrep::LiveGrep::new_no_ignore())),
        Box::pin(|| f(livegrep::LiveGrepF)),
        Box::pin(|| f(visits::Visits::all())),
        Box::pin(|| f(visits::Visits::project())),
        Box::pin(|| f(pr_list::GhPr::Open)),
        Box::pin(|| f(pr_list::GhPr::All)),
        Box::pin(|| f(pr_diff::PrDiff::new())),
        Box::pin(|| f(nas_worktree::NasWorktree)),
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

    fn fzf_bindings(&self) -> (ModeBindings, CallbackMap) {
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
        env: &'a Env,
        query: String,
        item: String, // currently selected item
    ) -> LoadStream<'a>;

    /// Preview the currently selected item
    fn preview<'a>(
        &'a self,
        env: &'a Env,
        win: &'a PreviewWindow,
        item: String,
    ) -> BoxFuture<'a, Result<PreviewResp>>;

    /// Execute the currently selected item
    /// (Optional. Intended to be used by the callback of fzf_bindings)
    fn execute<'a>(
        &'a self,
        _env: &'a Env,
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
        dyn for<'a> Fn(&'a (dyn ModeDef + Sync + Send), &'a Env, String, String) -> LoadStream<'a>
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
                String,
                String,
            ) -> BoxFuture<'a, Result<()>>
            + Sync
            + Send,
    >,
}

/// モード切替の共通処理: env.mode を更新 + fzf アクション生成 + POST
pub async fn do_change_mode(
    env: &Env,
    mode_name: &str,
    keep_query: bool,
    current_query: String,
    push_to_back_stack: bool,
) -> anyhow::Result<()> {
    let mode_info = env
        .mode_infos
        .get(mode_name)
        .ok_or_else(|| anyhow::anyhow!("unknown mode: {}", mode_name))?;

    let mut mode = env.mode.write().await;

    if push_to_back_stack {
        let old_mode_name = mode.current_mode_name().to_string();
        let cursor_pos = mode.take_pending_cursor_pos().unwrap_or(0);
        mode.push_back_stack(crate::env::BackStackEntry {
            mode_name: old_mode_name,
            query: current_query,
            cursor_pos,
        });
    }

    mode.set_current_mode_name(mode_name.to_string());

    let mut actions: Vec<ModeAction> = vec![
        ModeAction::Reload("default".to_string()),
        ModeAction::Fzf(fzf::Action::ChangePrompt(mode_info.prompt.clone())),
    ];

    if !keep_query {
        actions.push(ModeAction::Fzf(fzf::Action::ClearQuery));
    }

    if mode.sort_enabled() != mode_info.wants_sort {
        actions.push(ModeAction::Fzf(fzf::Action::ToggleSort));
    }
    mode.set_sort_enabled(mode_info.wants_sort);

    if mode_info.disable_search {
        actions.push(ModeAction::Fzf(fzf::Action::DisableSearch));
    } else {
        actions.push(ModeAction::Fzf(fzf::Action::EnableSearch));
    }

    actions.push(ModeAction::Fzf(fzf::Action::ChangePreviewWindow(
        mode_info
            .custom_preview_window
            .clone()
            .unwrap_or_else(|| "right:50%:noborder".to_string()),
    )));

    actions.push(ModeAction::Fzf(fzf::Action::DeselectAll));

    env.post_fzf_actions(&actions).await
}

pub mod config_builder {
    #![allow(dead_code)]
    use crate::env::Env;
    use crate::mode::lib::actions;
    use crate::mode::lib::item::ItemExtractor;
    use crate::mode::ModeAction;
    use crate::mode::ModeDef;
    use crate::nvim::NeovimExt;
    use crate::utils::fzf;
    use anyhow::Result;
    use futures::future::BoxFuture;
    use futures::FutureExt;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static GENSYM_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn gensym() -> String {
        let id = GENSYM_COUNTER.fetch_add(1, Ordering::Relaxed);
        format!("cb{id}")
    }

    pub struct ConfigBuilder {
        pub callback_map: super::CallbackMap,
    }

    impl ConfigBuilder {
        pub fn new() -> Self {
            ConfigBuilder {
                callback_map: super::CallbackMap::empty(),
            }
        }

        pub fn execute<F>(&mut self, callback: F) -> ModeAction
        where
            for<'a> F: Fn(
                    &'a (dyn ModeDef + Sync + Send),
                    &'a Env,
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
            ModeAction::Execute(name)
        }

        pub fn execute_silent<F>(&mut self, callback: F) -> ModeAction
        where
            for<'a> F: Fn(
                    &'a (dyn ModeDef + Sync + Send),
                    &'a Env,
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
            ModeAction::ExecuteSilent(name)
        }

        pub fn reload(&mut self) -> ModeAction {
            ModeAction::Reload("default".to_string())
        }

        pub fn reload_with<F>(&mut self, callback: F) -> ModeAction
        where
            for<'a> F: Fn(
                    &'a (dyn ModeDef + Sync + Send),
                    &'a Env,
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
            ModeAction::Reload(name)
        }

        pub fn change_mode(&mut self, mode: impl Into<String>, keep_query: bool) -> ModeAction {
            let mode_name = mode.into();
            self.execute_silent(move |_mode_def, env, query, _item| {
                let mode_name = mode_name.clone();
                let query = query.clone();
                async move { super::do_change_mode(env, &mode_name, keep_query, query, true).await }
                    .boxed()
            })
        }

        pub fn change_prompt(&self, prompt: impl Into<String>) -> ModeAction {
            ModeAction::Fzf(fzf::Action::ChangePrompt(prompt.into()))
        }

        pub fn toggle_sort(&self) -> ModeAction {
            ModeAction::Fzf(fzf::Action::ToggleSort)
        }

        pub fn clear_query(&self) -> ModeAction {
            ModeAction::Fzf(fzf::Action::ClearQuery)
        }

        pub fn clear_screen(&self) -> ModeAction {
            ModeAction::Fzf(fzf::Action::ClearScreen)
        }

        pub fn first(&self) -> ModeAction {
            ModeAction::Fzf(fzf::Action::First)
        }

        pub fn toggle(&self) -> ModeAction {
            ModeAction::Fzf(fzf::Action::Toggle)
        }

        pub fn raw(&self, cmd: impl Into<String>) -> ModeAction {
            ModeAction::Fzf(fzf::Action::Raw(cmd.into()))
        }

        pub fn execute_as<M, F>(&mut self, callback: F) -> ModeAction
        where
            M: ModeDef + Send + Sync + 'static,
            F: for<'a> Fn(&'a M, &'a Env, String, String) -> BoxFuture<'a, Result<()>>
                + Send
                + Sync
                + 'static,
        {
            self.execute(move |mode_dyn, config, query, item| {
                let mode = mode_dyn
                    .as_any()
                    .downcast_ref::<M>()
                    .expect("ConfigBuilder type mismatch: callback registered for wrong mode");
                callback(mode, config, query, item)
            })
        }

        pub fn execute_silent_as<M, F>(&mut self, callback: F) -> ModeAction
        where
            M: ModeDef + Send + Sync + 'static,
            F: for<'a> Fn(&'a M, &'a Env, String, String) -> BoxFuture<'a, Result<()>>
                + Send
                + Sync
                + 'static,
        {
            self.execute_silent(move |mode_dyn, config, query, item| {
                let mode = mode_dyn
                    .as_any()
                    .downcast_ref::<M>()
                    .expect("ConfigBuilder type mismatch: callback registered for wrong mode");
                callback(mode, config, query, item)
            })
        }

        pub fn reload_with_as<M, F>(&mut self, callback: F) -> ModeAction
        where
            M: ModeDef + Send + Sync + 'static,
            for<'a> F:
                Fn(&'a M, &'a Env, String, String) -> super::LoadStream<'a> + Send + Sync + 'static,
        {
            self.reload_with(move |mode_dyn, config, query, item| {
                let mode = mode_dyn
                    .as_any()
                    .downcast_ref::<M>()
                    .expect("ConfigBuilder type mismatch: callback registered for wrong mode");
                callback(mode, config, query, item)
            })
        }

        fn gen_name(&mut self) -> String {
            gensym()
        }

        /// Neovim でファイルを開く execute アクションを生成
        pub fn open_nvim<E: ItemExtractor>(&mut self, extractor: E, tabedit: bool) -> ModeAction {
            self.execute(move |_mode, config, _query, item| {
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
        ) -> ModeAction {
            self.execute_silent(move |_mode, config, _query, item| {
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
        pub fn open_vscode<E: ItemExtractor>(&mut self, extractor: E) -> ModeAction {
            self.execute(move |_mode, config, _query, item| {
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
        pub fn yank_file<E: ItemExtractor>(&mut self, extractor: E) -> ModeAction {
            self.execute_silent(move |_mode, _config, _query, item| {
                let e = extractor.clone();
                async move { actions::yank(e.file(&item)?).await }.boxed()
            })
        }
    }

    #[macro_export]
    macro_rules! bindings {
        ($builder:ident <= $init:expr,
         $($k:expr => [ $($v:expr),* $(,)? ] ),* $(,)?) => {{
            let (bindings, callback_map) = $init;
            let mut $builder = $crate::mode::config_builder::ConfigBuilder::new();
            $builder.callback_map = callback_map;
            let bindings = bindings.merge(
                $crate::mode::ModeBindings(core::convert::From::from([$(
                    ($k.to_string(), vec![$($v),*]),
                )*]))
            );
            (bindings, $builder.callback_map)
        }};
    }
    pub use bindings;

    #[macro_export]
    macro_rules! execute {
        ($builder:ident, |$mode:ident, $config:ident, $query:ident, $item:ident| $v:expr) => {
            $builder
                .execute_as::<Self, _>(|$mode, $config, $query, $item| async move { $v }.boxed())
        };
    }
    pub use execute;

    #[macro_export]
    macro_rules! execute_silent {
        ($builder:ident, |$mode:ident, $config:ident, $query:ident, $item:ident| $v:expr) => {
            $builder.execute_silent_as::<Self, _>(|$mode, $config, $query, $item| {
                async move { $v }.boxed()
            })
        };
    }
    pub use execute_silent;

    #[macro_export]
    macro_rules! select_and_execute {
        // 条件付き + 後続あり
        (@arms [$($items:tt)*] [$($arms:tt)*]
         $k:expr , when $cond:expr => $v:expr , $($rest:tt)*) => {
            select_and_execute!(@arms
                [$($items)* ($k, $cond),]
                [$($arms)* $k => { $v }]
                $($rest)*)
        };
        // 無条件 + 後続あり
        (@arms [$($items:tt)*] [$($arms:tt)*]
         $k:expr => $v:expr , $($rest:tt)*) => {
            select_and_execute!(@arms
                [$($items)* ($k, true),]
                [$($arms)* $k => { $v }]
                $($rest)*)
        };
        // 条件付き 末尾
        (@arms [$($items:tt)*] [$($arms:tt)*]
         $k:expr , when $cond:expr => $v:expr) => {
            select_and_execute!(@arms
                [$($items)* ($k, $cond),]
                [$($arms)* $k => { $v }])
        };
        // 無条件 末尾
        (@arms [$($items:tt)*] [$($arms:tt)*]
         $k:expr => $v:expr) => {
            select_and_execute!(@arms
                [$($items)* ($k, true),]
                [$($arms)* $k => { $v }])
        };
        // ベースケース: コード生成
        (@arms [$(($k_item:expr, $cond_item:expr),)*] [$($arms:tt)*]) => {{
            let __items: Vec<&str> = [$(($k_item, $cond_item)),*]
                .into_iter()
                .filter(|(_, cond)| *cond)
                .map(|(k, _)| k)
                .collect();
            match &*$crate::utils::fzf::select(__items).await? {
                $($arms)*
                _ => { Ok(()) }
            }
        }};
        // エントリポイント
        ($builder:ident, |$mode:ident, $config:ident, $query:ident, $item:ident|
         $($rest:tt)*) => {
            $builder.execute_as::<Self, _>(|$mode, $config, $query, $item| async move {
                select_and_execute!(@arms [] [] $($rest)*)
            }.boxed())
        };
    }
    pub use select_and_execute;

    pub fn default_bindings() -> (super::ModeBindings, super::CallbackMap) {
        bindings! {
            b <= (super::ModeBindings::empty(), super::CallbackMap::empty()),
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
                b.execute_silent(|_mode_def, env, _query, _item| {
                    async move {
                        let entry = {
                            let mut mode = env.mode.write().await;
                            mode.pop_back_stack()
                        };
                        match entry {
                            None => Ok(()),
                            Some(entry) => {
                                let restore_query = entry.query.clone();
                                let restore_pos = entry.cursor_pos;
                                super::do_change_mode(
                                    env,
                                    &entry.mode_name,
                                    true,
                                    String::new(),
                                    false,
                                )
                                .await?;
                                env.post_fzf_actions(&[
                                    super::ModeAction::Fzf(fzf::Action::Raw(format!(
                                        "change-query({})",
                                        restore_query
                                    ))),
                                    super::ModeAction::Fzf(fzf::Action::Raw(format!(
                                        "pos({})",
                                        restore_pos
                                    ))),
                                ])
                                .await?;
                                Ok(())
                            }
                        }
                    }
                    .boxed()
                }),
            ],
            "alt-u" => [
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
            "alt-n" => [
                b.change_mode(super::nas_worktree::NasWorktree.name(), false),
            ],
            "ctrl-u" => [
                b.execute_silent(|_mode, _env, _query, _item| {
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
                b.execute_silent(|_mode, _env, _query, item| {
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
                b.execute_silent(|_mode, env, _query, _item| {
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
