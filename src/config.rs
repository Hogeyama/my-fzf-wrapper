use std::collections::HashMap;

use crate::mode;
use crate::mode::CallbackMap;
use crate::mode::MkMode;
use crate::mode::Mode;
use crate::mode::ModeDef;

pub struct Config {
    pub myself: String,
    pub socket: String,
    pub log_file: String,
    pub initial_mode: String,
    pub modes: Vec<(String, MkMode)>,
}

impl Config {
    pub fn get_mode_names(&self) -> Vec<&str> {
        self.modes.iter().map(|(name, _)| name.as_str()).collect()
    }

    /// 全モードを構築し、各モードの CallbackMap も生成する
    pub fn build_all_modes(&self) -> HashMap<String, (Mode, CallbackMap)> {
        self.modes
            .iter()
            .map(|(name, mk_mode)| {
                let mode = mk_mode();
                let callbacks = mode.callbacks();
                (name.clone(), (mode, callbacks))
            })
            .collect()
    }
}

pub fn new(myself: String, socket: String, log_file: String) -> Config {
    let initial_mode = mode::menu::Menu.name().to_string();
    let modes = mode::all_modes();
    Config {
        myself,
        socket,
        log_file,
        initial_mode,
        modes,
    }
}
