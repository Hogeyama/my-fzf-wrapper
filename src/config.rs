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
                        callback: Box::new(|mode_def, config, state, query, item| {
                            mode_def.load(config, state, query, item)
                        }),
                    },
                );
                callback_map.preview.insert(
                    "default".to_string(),
                    mode::PreviewCallback {
                        callback: Box::new(|mode_def, config, win, item| {
                            mode_def.preview(config, win, item)
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

        // 固定バインディング (サーバー往復不要)
        let fixed_keys: HashSet<&str> = ["shift-right"].iter().cloned().collect();

        let mut bindings = HashMap::new();

        // 固定バインディング: shift-right
        bindings.insert(
            "shift-right".to_string(),
            vec![ModeAction::Fzf(crate::utils::fzf::Action::Raw(
                "change-preview-window[bottom:90%:border-top|right:50%:noborder]".to_string(),
            ))],
        );

        // 全モード依存キーを execute-silent 経由で dispatch
        for key in &all_keys {
            if fixed_keys.contains(key.as_str()) {
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
