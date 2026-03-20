use crate::method::LoadResp;

pub struct State {
    pub last_load_resp: Option<LoadResp>,
    current_mode_name: String,
    sort_enabled: bool,
}

impl State {
    pub fn new(initial_mode: String, initial_sort: bool) -> Self {
        State {
            last_load_resp: None,
            current_mode_name: initial_mode,
            sort_enabled: initial_sort,
        }
    }

    pub fn current_mode_name(&self) -> &str {
        &self.current_mode_name
    }

    pub fn sort_enabled(&self) -> bool {
        self.sort_enabled
    }

    pub(crate) fn set_current_mode_name(&mut self, name: String) {
        self.current_mode_name = name;
    }

    pub(crate) fn set_sort_enabled(&mut self, v: bool) {
        self.sort_enabled = v;
    }
}
