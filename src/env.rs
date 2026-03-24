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
    fn mode_state_accessors() {
        let mode = ModeState::new("menu".into(), false);
        assert_eq!(mode.current_mode_name(), "menu");
        assert!(!mode.sort_enabled());
    }
}
