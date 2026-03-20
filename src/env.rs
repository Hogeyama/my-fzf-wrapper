use std::collections::HashMap;
use std::sync::Arc;

use crate::config::Config;
use crate::mode::ModeAction;
use crate::nvim::Neovim;
use crate::utils::fzf;

pub struct Env {
    pub config: Config,
    pub nvim: Neovim,
    pub fzf_client: Arc<fzf::FzfClient>,
    pub mode_infos: Arc<HashMap<String, ModeInfo>>,
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
