use std::pin::Pin;

use crate::mode;
use crate::mode::Mode;

pub struct Config {
    pub myself: String,
    pub socket: String,
    pub log_file: String,
    pub initial_mode: String,
    pub modes: Vec<(String, MkMode)>,
}

// modeの切り替えの度に初期化するため複雑になっている。もっと良い方法がありそう。
type MkMode = Pin<Box<dyn (Fn() -> Mode) + Send + Sync>>;

impl Config {
    pub fn get_initial_mode(&self) -> Mode {
        self.get_mode(&self.initial_mode)
    }

    pub fn get_mode(&self, mode: impl Into<String>) -> Mode {
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
}

pub fn new(myself: String, socket: String, log_file: String) -> Config {
    fn f(mode_def: Box<dyn mode::ModeDef + Sync + Send>) -> Mode {
        Mode { mode_def }
    }
    let initial_mode = "menu".to_string();
    let modes: Vec<MkMode> = vec![
        Box::pin(|| f(Box::new(mode::menu::Menu))),
        Box::pin(|| f(Box::new(mode::fd::Fd))),
        Box::pin(|| f(Box::new(mode::buffer::Buffer))),
        Box::pin(|| f(Box::new(mode::zoxide::Zoxide))),
        Box::pin(|| f(Box::new(mode::mru::Mru))),
        Box::pin(|| f(Box::new(mode::diagnostics::Diagnostics))),
        Box::pin(|| f(Box::new(mode::browser_history::BrowserHistory))),
        Box::pin(|| f(Box::new(mode::git_branch::GitBranch))),
        Box::pin(|| f(Box::new(mode::git_log::GitLog::Head))),
        Box::pin(|| f(Box::new(mode::git_log::GitLog::All))),
        Box::pin(|| f(Box::new(mode::git_reflog::GitReflog))),
        Box::pin(|| f(Box::new(mode::git_status::GitStatus))),
        Box::pin(|| f(Box::new(mode::git_diff::GitDiff::new()))),
        Box::pin(|| f(Box::new(mode::nvim_session::NeovimSession))),
        Box::pin(|| f(Box::new(mode::livegrep::LiveGrep::new()))),
        Box::pin(|| f(Box::new(mode::livegrep::LiveGrep::new_no_ignore()))),
        Box::pin(|| f(Box::new(mode::livegrep::LiveGrepF))),
    ];
    let modes = modes
        .into_iter()
        .map(|mk_mode| (mk_mode().name().to_string(), mk_mode))
        .collect();
    Config {
        myself,
        socket,
        log_file,
        initial_mode,
        modes,
    }
}
