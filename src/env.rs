use crate::config::Config;
use crate::nvim::Neovim;

pub struct Env {
    pub config: Config,
    pub nvim: Neovim,
}
