use std::pin::Pin;

use crate::mode;
use crate::types::Mode;

// TODO key binding を含める。fzf::new がそれを受け取れるようにする。
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
    Config {
        modes: vec![
            (
                "menu".to_string(), //
                Box::pin(|| Box::new(mode::menu::new())),
            ),
            (
                "fd".to_string(), //
                Box::pin(|| Box::new(mode::fd::new())),
            ),
            (
                "rg".to_string(), //
                Box::pin(|| Box::new(mode::rg::new())),
            ),
            (
                "buffer".to_string(),
                Box::pin(|| Box::new(mode::buffer::new())),
            ),
            (
                "zoxide".to_string(),
                Box::pin(|| Box::new(mode::zoxide::new())),
            ),
            (
                "mru".to_string(), //
                Box::pin(|| Box::new(mode::mru::new())),
            ),
            (
                "diagnostics".to_string(),
                Box::pin(|| Box::new(mode::diagnostics::new())),
            ),
            (
                "browser-history".to_string(),
                Box::pin(|| Box::new(mode::browser_history::new())),
            ),
            (
                "git-branch".to_string(),
                Box::pin(|| Box::new(mode::git_branch::new())),
            ),
            (
                "git-log".to_string(),
                Box::pin(|| Box::new(mode::git_log::new())),
            ),
            (
                "nvim-session".to_string(),
                Box::pin(|| Box::new(mode::nvim_session::new())),
            ),
        ],
    }
}
