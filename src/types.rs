use std::collections::HashMap;

use crate::{
    method::{LoadParam, LoadResp, PreviewResp, RunOpts, RunResp},
    nvim::Neovim,
};

use futures::future::BoxFuture;

pub struct State {
    // 直近の load の引数を reload 用に保持しておく
    pub last_load_param: LoadParam,
    // なんだばほ
    pub last_load_resp: Option<LoadResp>,
    // neovim instance
    pub nvim: Neovim,

    pub mode: Option<Box<dyn Mode + Sync + Send>>,

    pub keymap: HashMap<String, serde_json::Value>,
}

pub trait Mode {
    /// The name of the mode
    fn name(&self) -> &'static str;

    /// Load items into fzf
    fn load<'a>(&'a mut self, state: &'a mut State, arg: Vec<String>) -> BoxFuture<'a, LoadResp>;

    /// Run command with the selected item
    fn preview<'a>(&'a mut self, state: &'a mut State, item: String) -> BoxFuture<'a, PreviewResp>;

    /// Run command with the selected item
    fn run<'a>(
        &'a mut self,
        state: &'a mut State,
        item: String,
        opts: RunOpts,
    ) -> BoxFuture<'a, RunResp>;
}
