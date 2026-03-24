use std::collections::HashMap;
use std::sync::Arc;

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
    /// load 中に長時間 write ロックが保持される
    pub load: Arc<RwLock<LoadState>>,
    /// モード状態: 独立ロックで load とは競合しない
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

pub struct LoadState {
    pub last_load_resp: Option<LoadResp>,
}

pub struct ModeState {
    current_mode_name: String,
    sort_enabled: bool,
}

impl ModeState {
    pub fn new(initial_mode: String, initial_sort: bool) -> Self {
        ModeState {
            current_mode_name: initial_mode,
            sort_enabled: initial_sort,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_and_mode_locks_are_independent() {
        let load = Arc::new(RwLock::new(LoadState { last_load_resp: None }));
        let mode = Arc::new(RwLock::new(ModeState {
            current_mode_name: "menu".into(),
            sort_enabled: true,
        }));
        let _load_guard = load.try_write().unwrap();
        // load の write ロック中でも mode は読める
        let mode_guard = mode.try_read().unwrap();
        assert_eq!(mode_guard.current_mode_name(), "menu");
    }

    #[test]
    fn mode_state_accessors() {
        let mode = Arc::new(RwLock::new(ModeState {
            current_mode_name: "menu".into(),
            sort_enabled: false,
        }));
        let mut m = mode.try_write().unwrap();
        m.set_current_mode_name("fd".into());
        assert_eq!(m.current_mode_name(), "fd");
        assert!(!m.sort_enabled());
        m.set_sort_enabled(true);
        assert!(m.sort_enabled());
    }

    #[test]
    fn load_state_last_resp() {
        let load = Arc::new(RwLock::new(LoadState { last_load_resp: None }));
        let mut l = load.try_write().unwrap();
        l.last_load_resp = Some(LoadResp {
            header: Some("[test]".into()),
            items: vec!["a".into(), "b".into()],
            is_last: true,
        });
        let resp = l.last_load_resp.as_ref().unwrap();
        assert_eq!(resp.items.len(), 2);
        assert!(resp.is_last);
    }
}
