use crate::mode;
use crate::mode::MkMode;
use crate::mode::Mode;
use crate::mode::ModeDef;
use crate::nvim::Neovim;

pub struct Config {
    pub myself: String,
    pub socket: String,
    pub log_file: String,
    pub initial_mode: String,
    pub nvim: Neovim,
    pub modes: Vec<(String, MkMode)>,
}

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

pub fn new(myself: String, nvim: Neovim, socket: String, log_file: String) -> Config {
    let initial_mode = mode::menu::Menu.name().to_string();
    let modes = mode::all_modes();
    Config {
        myself,
        nvim,
        socket,
        log_file,
        initial_mode,
        modes,
    }
}
