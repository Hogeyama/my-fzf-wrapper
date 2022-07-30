use std::borrow::Cow;
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
    pub fn get_mode<'a, 'b>(
        &'a self,
        mode: impl Into<Cow<'b, str>>,
    ) -> Box<dyn Mode + Send + Sync> {
        let mode = mode.into().into_owned();
        self.modes.get(&mode).unwrap()()
    }
}
