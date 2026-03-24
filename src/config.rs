use std::collections::HashMap;

use crate::mode;
use crate::mode::CallbackMap;
use crate::mode::MkMode;
use crate::mode::ModeAction;
use crate::mode::ModeBindings;
use crate::mode::Mode;
use crate::mode::ModeDef;

pub struct Config {
    pub myself: String,
    pub socket: String,
    pub log_file: String,
    pub initial_mode: String,
    pub modes: Vec<(String, MkMode)>,
}

/// 各モードのデータを保持する構造体
pub struct ModeEntry {
    pub mode: Mode,
    pub callbacks: CallbackMap,
    /// key → rendered fzf action string (e.g. "execute[fzfw execute callback3 {q} {}]")
    pub rendered_bindings: HashMap<String, String>,
}

impl Config {
    pub fn get_mode_names(&self) -> Vec<&str> {
        self.modes.iter().map(|(name, _)| name.as_str()).collect()
    }

    /// ModeBindings を myself 付きでレンダリングする
    pub fn render_mode_bindings(&self, bindings: &ModeBindings) -> HashMap<String, String> {
        bindings
            .0
            .iter()
            .map(|(key, actions)| {
                let rendered = actions
                    .iter()
                    .map(|a| a.clone().into_fzf_action(&self.myself).render())
                    .collect::<Vec<_>>()
                    .join("+");
                (key.clone(), rendered)
            })
            .collect()
    }

    /// 全モードを構築し、CallbackMap と rendered_bindings も生成する
    pub fn build_all_modes(&self) -> HashMap<String, ModeEntry> {
        self.modes
            .iter()
            .map(|(name, mk_mode)| {
                let mode = mk_mode();
                let (bindings, mut callback_map) = mode.mode_def.fzf_bindings();

                // default load/preview callbacks を登録
                callback_map.load.insert(
                    "default".to_string(),
                    mode::LoadCallback {
                        callback: Box::new(|mode_def, env, query, item| {
                            mode_def.load(env, query, item)
                        }),
                    },
                );
                callback_map.preview.insert(
                    "default".to_string(),
                    mode::PreviewCallback {
                        callback: Box::new(|mode_def, env, win, item| {
                            mode_def.preview(env, win, item)
                        }),
                    },
                );

                let rendered_bindings = self.render_mode_bindings(&bindings);

                (
                    name.clone(),
                    ModeEntry {
                        mode,
                        callbacks: callback_map,
                        rendered_bindings,
                    },
                )
            })
            .collect()
    }

    /// 全モードの全キーを集約し、execute-silent 経由の統合バインディングを生成
    pub fn build_unified_bindings(
        &self,
        all_modes: &HashMap<String, ModeEntry>,
    ) -> ModeBindings {
        use std::collections::HashSet;

        // 全モードで使われるキーを収集
        let mut all_keys: HashSet<String> = HashSet::new();
        for entry in all_modes.values() {
            for key in entry.rendered_bindings.keys() {
                all_keys.insert(key.clone());
            }
        }

        // 固定バインディング
        let mut bindings = HashMap::from([
            (
                "shift-right".to_string(),
                vec![ModeAction::Fzf(crate::utils::fzf::Action::Raw(
                    "change-preview-window[bottom:90%:border-top|right:50%:noborder]".to_string(),
                ))],
            ),
        ]);

        let fixed_keys: HashSet<String> = bindings.keys().cloned().collect();

        // 全モード依存キーを execute-silent 経由で dispatch
        for key in &all_keys {
            if fixed_keys.contains(key) {
                continue;
            }
            bindings.insert(
                key.clone(),
                vec![ModeAction::ExecuteSilent(format!("_key:{}", key))],
            );
        }

        ModeBindings(bindings)
    }
}

pub fn new(myself: String, socket: String, log_file: String) -> Config {
    let initial_mode = mode::menu::Menu.name().to_string();
    let modes = mode::all_modes();
    Config {
        myself,
        socket,
        log_file,
        initial_mode,
        modes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::fzf;

    fn test_config() -> Config {
        new("fzfw".into(), "/tmp/test.sock".into(), "/tmp/test.log".into())
    }

    #[test]
    fn get_mode_names_includes_menu() {
        let config = test_config();
        let names = config.get_mode_names();
        assert!(names.contains(&"menu"));
    }

    #[test]
    fn get_mode_names_includes_fd() {
        let config = test_config();
        let names = config.get_mode_names();
        assert!(names.contains(&"fd"));
    }

    #[test]
    fn render_mode_bindings_single_action() {
        let config = test_config();
        let bindings = ModeBindings(std::collections::HashMap::from([(
            "enter".to_string(),
            vec![ModeAction::Fzf(fzf::Action::First)],
        )]));
        let rendered = config.render_mode_bindings(&bindings);
        assert_eq!(rendered.get("enter").unwrap(), "first");
    }

    #[test]
    fn render_mode_bindings_multiple_actions_joined_with_plus() {
        let config = test_config();
        let bindings = ModeBindings(std::collections::HashMap::from([(
            "ctrl-r".to_string(),
            vec![
                ModeAction::Reload("default".into()),
                ModeAction::Fzf(fzf::Action::ClearScreen),
            ],
        )]));
        let rendered = config.render_mode_bindings(&bindings);
        let value = rendered.get("ctrl-r").unwrap();
        assert!(value.contains("reload["));
        assert!(value.contains("+clear-screen"));
    }

    #[test]
    fn build_all_modes_registers_default_callbacks() {
        let config = test_config();
        let all = config.build_all_modes();
        for (name, entry) in &all {
            assert!(
                entry.callbacks.load.contains_key("default"),
                "mode {} should have default load callback",
                name
            );
            assert!(
                entry.callbacks.preview.contains_key("default"),
                "mode {} should have default preview callback",
                name
            );
        }
    }

    #[test]
    fn build_unified_bindings_includes_shift_right() {
        let config = test_config();
        let all = config.build_all_modes();
        let unified = config.build_unified_bindings(&all);
        assert!(unified.0.contains_key("shift-right"));
    }

    #[test]
    fn build_unified_bindings_dispatches_via_execute_silent() {
        let config = test_config();
        let all = config.build_all_modes();
        let unified = config.build_unified_bindings(&all);
        // "enter" は多くのモードで使われるはず
        if let Some(actions) = unified.0.get("enter") {
            assert_eq!(actions.len(), 1);
            match &actions[0] {
                ModeAction::ExecuteSilent(name) => assert_eq!(name, "_key:enter"),
                other => panic!("expected ExecuteSilent, got {:?}", std::mem::discriminant(other)),
            }
        }
    }
}
