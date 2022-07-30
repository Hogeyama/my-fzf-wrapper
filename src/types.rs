use crate::{
    method::{LoadParam, LoadResp, PreviewResp, RunOpts, RunResp},
    nvim::Neovim,
};

use std::path::PathBuf;

use futures::future::BoxFuture;

pub struct State {
    // TODO なんでこれ持つ必要があるんだっけ。
    pub pwd: PathBuf,
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

struct Dummy {}
impl Mode for Dummy {
    fn name(&self) -> &'static str {
        panic!()
    }
    fn load<'a>(&mut self, _state: &'a mut State, _arg: Vec<String>) -> BoxFuture<'a, LoadResp> {
        panic!()
    }
    fn preview<'a>(&mut self, _state: &'a mut State, _item: String) -> BoxFuture<'a, PreviewResp> {
        panic!()
    }
    fn run<'a>(
        &mut self,
        _state: &'a mut State,
        _item: String,
        _opts: RunOpts,
    ) -> BoxFuture<'a, RunResp> {
        panic!()
    }
}
