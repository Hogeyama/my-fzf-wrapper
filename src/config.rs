use std::borrow::Cow;
use std::collections::HashMap;

use crate::types::Mode;

// TODO key binding を含める。fzf::new がそれを受け取れるようにする。
pub struct Config {
    pub modes: HashMap<String, Box<dyn Mode + Send + Sync>>,
}

// 参照ではなく所有が必要になった場合（もっといい方法もありそうだが……）:
// type MkMode = Pin<Box<dyn (Fn() -> Box<dyn Mode + Send + Sync>) + Send>>;

impl Config {
    pub fn get_mode<'a, 'b>(
        &'a self,
        mode: impl Into<Cow<'b, str>>,
    ) -> &'a Box<dyn Mode + Send + Sync> {
        let mode = mode.into().into_owned();
        self.modes.get(&mode).unwrap()
    }
}
