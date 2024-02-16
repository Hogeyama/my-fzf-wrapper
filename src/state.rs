use crate::{method::LoadResp, nvim::Neovim};

pub struct State {
    pub nvim: Neovim,

    pub last_load_resp: Option<LoadResp>,
}

impl State {
    pub fn new(nvim: Neovim) -> Self {
        State {
            nvim,
            last_load_resp: None,
        }
    }
}
