use std::sync::Arc;

use tokio::sync::RwLock;

use crate::method::LoadResp;

/// load 中に長時間 write ロックが保持されるフィールド
pub struct LoadState {
    pub last_load_resp: Option<LoadResp>,
}

/// モード状態: 独立ロックで load とは競合しない
pub struct ModeState {
    current_mode_name: String,
    sort_enabled: bool,
}

#[derive(Clone)]
pub struct State {
    pub load: Arc<RwLock<LoadState>>,
    pub mode: Arc<RwLock<ModeState>>,
}

impl State {
    pub fn new(initial_mode: String, initial_sort: bool) -> Self {
        State {
            load: Arc::new(RwLock::new(LoadState {
                last_load_resp: None,
            })),
            mode: Arc::new(RwLock::new(ModeState {
                current_mode_name: initial_mode,
                sort_enabled: initial_sort,
            })),
        }
    }
}

impl ModeState {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_has_initial_values() {
        let state = State::new("menu".into(), true);
        let mode = state.mode.try_read().unwrap();
        assert_eq!(mode.current_mode_name(), "menu");
        assert!(mode.sort_enabled());
        assert!(state.load.try_read().unwrap().last_load_resp.is_none());
    }

    #[test]
    fn set_current_mode_name() {
        let state = State::new("menu".into(), false);
        let mut mode = state.mode.try_write().unwrap();
        mode.set_current_mode_name("fd".into());
        assert_eq!(mode.current_mode_name(), "fd");
    }

    #[test]
    fn set_sort_enabled() {
        let state = State::new("menu".into(), false);
        let mut mode = state.mode.try_write().unwrap();
        assert!(!mode.sort_enabled());
        mode.set_sort_enabled(true);
        assert!(mode.sort_enabled());
    }

    #[test]
    fn last_load_resp_can_be_set() {
        let state = State::new("menu".into(), true);
        let mut load = state.load.try_write().unwrap();
        load.last_load_resp = Some(LoadResp {
            header: Some("[test]".into()),
            items: vec!["a".into(), "b".into()],
            is_last: true,
        });
        let resp = load.last_load_resp.as_ref().unwrap();
        assert_eq!(resp.items.len(), 2);
        assert!(resp.is_last);
    }

    #[test]
    fn load_and_mode_locks_are_independent() {
        let state = State::new("menu".into(), true);
        let _load = state.load.try_write().unwrap();
        // load の write ロック中でも mode は読める
        let mode = state.mode.try_read().unwrap();
        assert_eq!(mode.current_mode_name(), "menu");
    }
}
