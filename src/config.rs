use std::collections::HashMap;
use std::pin::Pin;

use crate::types::Mode;

// TODO key binding を含める。fzf::new がそれを受け取れるようにする。
pub struct Config {
    pub modes: HashMap<String, MkMode>,
}

// modeの切り替えの度に初期化するため複雑になっている。もっと良い方法がありそう。
pub type MkMode = Pin<Box<dyn (Fn() -> Box<dyn Mode + Send + Sync>) + Send + Sync>>;

impl Config {
    pub fn get_mode(&self, mode: impl Into<String>) -> Box<dyn Mode + Send + Sync> {
        let mode = mode.into();
        self.modes.get(&mode).unwrap()()
    }
}
