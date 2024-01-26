use std::collections::HashMap;

use crate::{method::LoadResp, nvim::Neovim};

pub struct State {
    pub nvim: Neovim,

    pub last_load_resp: Option<LoadResp>,

    // TODO remove
    pub keymap: HashMap<String, serde_json::Value>,
}

impl State {
    pub fn new(nvim: Neovim) -> Self {
        State {
            nvim,
            last_load_resp: None,
            keymap: HashMap::new(),
        }
    }
}
