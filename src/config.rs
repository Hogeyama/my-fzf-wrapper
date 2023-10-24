use std::pin::Pin;

use crate::mode;
use crate::types::Mode;

pub struct Config {
    modes: Vec<(String, MkMode)>,
}

// modeの切り替えの度に初期化するため複雑になっている。もっと良い方法がありそう。
type MkMode = Pin<Box<dyn (Fn() -> Box<dyn Mode + Send + Sync>) + Send + Sync>>;

impl Config {
    pub fn get_mode(&self, mode: impl Into<String>) -> Box<dyn Mode + Send + Sync> {
        let mode = mode.into();
        for (name, mk_mode) in &self.modes {
            if name == &mode {
                return mk_mode();
            }
        }
        panic!("unknown mode: {}", mode);
    }

    pub fn get_mode_names(&self) -> Vec<&str> {
        self.modes.iter().map(|(name, _)| name.as_str()).collect()
    }

    pub fn is_mode_valid(&self, mode: &str) -> bool {
        for (name, _) in &self.modes {
            if name == mode {
                return true;
            }
        }
        false
    }
}

pub fn new() -> Config {
    let modes: Vec<MkMode> = vec![
        Box::pin(|| Box::new(mode::menu::Menu::new())),
        Box::pin(|| Box::new(mode::fd::Fd::new())),
        Box::pin(|| Box::new(mode::buffer::Buffer::new())),
        Box::pin(|| Box::new(mode::zoxide::Zoxide::new())),
        Box::pin(|| Box::new(mode::mru::Mru::new())),
        Box::pin(|| Box::new(mode::diagnostics::Diagnostics::new())),
        Box::pin(|| Box::new(mode::browser_history::BrowserHistory::new())),
        Box::pin(|| Box::new(mode::git_branch::GitBranch::new())),
        Box::pin(|| Box::new(mode::git_log::GitLog::new())),
        Box::pin(|| Box::new(mode::git_reflog::GitReflog::new())),
        Box::pin(|| Box::new(mode::nvim_session::NeovimSession::new())),
        Box::pin(|| Box::new(mode::livegrep::LiveGrep::new())),
        Box::pin(|| Box::new(mode::livegrep::LiveGrepF::new())),
    ];
    let modes = modes
        .into_iter()
        .map(|mk_mode| (mk_mode().name().to_string(), mk_mode))
        .collect();
    Config { modes }
}
