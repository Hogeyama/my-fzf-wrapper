use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;
use tokio::sync::RwLock;

use crate::config::Config;
use crate::method::LoadResp;
use crate::mode::ModeAction;
use crate::nvim::Neovim;
use crate::utils::fzf;

pub struct Env {
    pub config: Config,
    pub nvim: Neovim,
    pub fzf_client: Arc<fzf::FzfClient>,
    pub mode_infos: Arc<HashMap<String, ModeInfo>>,
    /// 直近の load 結果
    pub last_load_resp: Mutex<Option<LoadResp>>,
    /// モード状態
    pub mode: Arc<RwLock<ModeState>>,
}

impl Env {
    /// ModeAction スライスを myself 付きでレンダリングして fzf に POST する
    pub async fn post_fzf_actions(&self, actions: &[ModeAction]) -> anyhow::Result<()> {
        let rendered = actions
            .iter()
            .map(|a| a.clone().into_fzf_action(&self.config.myself).render())
            .collect::<Vec<_>>()
            .join("+");
        self.fzf_client.post_action(&rendered).await
    }
}

/// モード切替時に必要なメタデータ (起動時に全モードから事前計算)
pub struct ModeInfo {
    pub prompt: String,
    pub wants_sort: bool,
    pub disable_search: bool,
    pub custom_preview_window: Option<String>,
}

#[allow(dead_code)]
const MAX_BACK_STACK_DEPTH: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct BackStackEntry {
    pub mode_name: String,
    pub query: String,
    pub cursor_pos: usize,
}

pub struct ModeState {
    current_mode_name: String,
    sort_enabled: bool,
    #[allow(dead_code)]
    back_stack: Vec<BackStackEntry>,
    #[allow(dead_code)]
    pending_cursor_pos: Option<usize>,
}

impl ModeState {
    pub fn new(initial_mode: String, initial_sort: bool) -> Self {
        ModeState {
            current_mode_name: initial_mode,
            sort_enabled: initial_sort,
            back_stack: Vec::new(),
            pending_cursor_pos: None,
        }
    }

    pub fn current_mode_name(&self) -> &str {
        &self.current_mode_name
    }

    pub fn sort_enabled(&self) -> bool {
        self.sort_enabled
    }

    pub fn set_current_mode_name(&mut self, name: String) {
        self.current_mode_name = name;
    }

    pub fn set_sort_enabled(&mut self, v: bool) {
        self.sort_enabled = v;
    }

    #[allow(dead_code)]
    pub fn push_back_stack(&mut self, entry: BackStackEntry) {
        if self.back_stack.len() >= MAX_BACK_STACK_DEPTH {
            self.back_stack.remove(0);
        }
        self.back_stack.push(entry);
    }

    #[allow(dead_code)]
    pub fn pop_back_stack(&mut self) -> Option<BackStackEntry> {
        self.back_stack.pop()
    }

    #[allow(dead_code)]
    pub fn set_pending_cursor_pos(&mut self, pos: usize) {
        self.pending_cursor_pos = Some(pos);
    }

    #[allow(dead_code)]
    pub fn take_pending_cursor_pos(&mut self) -> Option<usize> {
        self.pending_cursor_pos.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_state_accessors() {
        let mode = ModeState::new("menu".into(), false);
        assert_eq!(mode.current_mode_name(), "menu");
        assert!(!mode.sort_enabled());
    }

    #[test]
    fn back_stack_push_and_pop() {
        let mut mode = ModeState::new("menu".into(), false);
        let entry = BackStackEntry {
            mode_name: "fd".into(),
            query: "hello".into(),
            cursor_pos: 5,
        };
        mode.push_back_stack(entry.clone());
        let popped = mode.pop_back_stack().unwrap();
        assert_eq!(popped.mode_name, "fd");
        assert_eq!(popped.query, "hello");
        assert_eq!(popped.cursor_pos, 5);
    }

    #[test]
    fn back_stack_empty_pop_returns_none() {
        let mut mode = ModeState::new("menu".into(), false);
        assert!(mode.pop_back_stack().is_none());
    }

    #[test]
    fn back_stack_depth_limit() {
        let mut mode = ModeState::new("menu".into(), false);
        for i in 0..MAX_BACK_STACK_DEPTH + 5 {
            mode.push_back_stack(BackStackEntry {
                mode_name: format!("mode-{}", i),
                query: String::new(),
                cursor_pos: i,
            });
        }
        assert_eq!(mode.back_stack.len(), MAX_BACK_STACK_DEPTH);
        // The oldest entries (0..5) should have been removed
        let oldest = mode.back_stack.first().unwrap();
        assert_eq!(oldest.mode_name, "mode-5");
    }

    #[test]
    fn pending_cursor_pos_take() {
        let mut mode = ModeState::new("menu".into(), false);
        mode.set_pending_cursor_pos(42);
        assert_eq!(mode.take_pending_cursor_pos(), Some(42));
        assert_eq!(mode.take_pending_cursor_pos(), None);
    }
}
