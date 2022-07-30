use crate::{
    method::{LoadParam, LoadResp, PreviewResp, RunOpts, RunResp},
    nvim::Neovim,
};

use std::path::PathBuf;

use futures::future::BoxFuture;

pub struct State<'a> {
    // TODO なんでこれ持つ必要があるんだっけ。
    pub pwd: PathBuf,
    // 現在の mode
    pub mode: &'a Box<dyn Mode + Send + Sync>,
    // 直近の load の引数を reload 用に保持しておく
    pub last_load: LoadParam,
    // neovim instance
    pub nvim: Neovim,
}

pub trait Mode {
    /// The name of the mode
    fn name(&self) -> &'static str;

    // &mut self にしたくなるときが来るかもしれない
    /// Load items into fzf
    fn load<'a>(&self, state: &'a mut State, arg: Vec<String>) -> BoxFuture<'a, LoadResp>;

    /// Run command with the selected item
    fn preview<'a>(&self, state: &'a mut State, item: String) -> BoxFuture<'a, PreviewResp>;

    /// Run command with the selected item
    fn run<'a>(&self, state: &'a mut State, item: String, opts: RunOpts) -> BoxFuture<'a, RunResp>;
}
