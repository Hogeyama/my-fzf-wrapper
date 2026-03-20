use std::collections::HashMap;
use std::sync::Arc;

use crate::config::Config;
use crate::nvim::Neovim;
use crate::utils::fzf;

pub struct Env {
    pub config: Config,
    pub nvim: Neovim,
    pub fzf_client: Arc<fzf::FzfClient>,
    pub mode_infos: Arc<HashMap<String, ModeInfo>>,
}

/// モード切替時に必要なメタデータ (起動時に全モードから事前計算)
pub struct ModeInfo {
    pub prompt: String,
    pub wants_sort: bool,
    pub disable_search: bool,
    pub custom_preview_window: Option<String>,
}
