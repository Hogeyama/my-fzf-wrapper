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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_has_initial_values() {
        let state = State::new("menu".into(), true);
        assert_eq!(state.current_mode_name(), "menu");
        assert!(state.sort_enabled());
        assert!(state.last_load_resp.is_none());
    }

    #[test]
    fn set_current_mode_name() {
        let mut state = State::new("menu".into(), false);
        state.set_current_mode_name("fd".into());
        assert_eq!(state.current_mode_name(), "fd");
    }

    #[test]
    fn set_sort_enabled() {
        let mut state = State::new("menu".into(), false);
        assert!(!state.sort_enabled());
        state.set_sort_enabled(true);
        assert!(state.sort_enabled());
    }

    #[test]
    fn last_load_resp_can_be_set() {
        let mut state = State::new("menu".into(), true);
        state.last_load_resp = Some(LoadResp {
            header: Some("[test]".into()),
            items: vec!["a".into(), "b".into()],
            is_last: true,
        });
        let resp = state.last_load_resp.as_ref().unwrap();
        assert_eq!(resp.items.len(), 2);
        assert!(resp.is_last);
    }
}
