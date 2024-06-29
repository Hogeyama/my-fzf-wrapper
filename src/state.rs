use crate::method::LoadResp;

pub struct State {
    pub last_load_resp: Option<LoadResp>,
}

impl State {
    pub fn new() -> Self {
        State {
            last_load_resp: None,
        }
    }
}
