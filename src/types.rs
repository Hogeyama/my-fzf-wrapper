use std::collections::HashMap;

use crate::{
    external_command::fzf,
    method::{LoadParam, LoadResp, PreviewResp, RunOpts, RunResp},
    nvim::Neovim,
};

use futures::future::BoxFuture;

pub struct State {
    pub nvim: Neovim,

    pub last_load_param: LoadParam,

    pub last_load_resp: Option<LoadResp>,

    pub mode: Option<Box<dyn Mode + Sync + Send>>,

    pub keymap: HashMap<String, serde_json::Value>,
}

impl State {
    pub fn new(nvim: Neovim, mode: Box<dyn Mode + Sync + Send>) -> Self {
        let mode_name = mode.name().to_string();
        State {
            nvim,
            mode: Some(mode),
            last_load_param: LoadParam {
                mode: mode_name,
                args: vec![],
            },
            last_load_resp: None,
            keymap: HashMap::new(),
        }
    }
}

pub trait Mode {
    /// The name of the mode
    fn name(&self) -> &'static str;

    fn new() -> Self
    where
        Self: Sized;

    fn fzf_config(&self, config: FzfConfig) -> fzf::Config {
        fzf::Config {
            myself: config.myself,
            socket: config.socket,
            log_file: config.log_file,
            load: [vec![self.name().to_string(), "--".to_string()], config.args].concat(),
            initial_prompt: self.fzf_prompt(),
            initial_query: config.initial_query,
            bindings: self.fzf_bindings(),
            extra_opts: self.fzf_extra_opts(),
        }
    }

    fn fzf_prompt(&self) -> String {
        format!("{}>", self.name())
    }

    fn fzf_bindings(&self) -> fzf::Bindings {
        default_bindings()
    }

    fn fzf_extra_opts(&self) -> Vec<String> {
        vec![]
    }

    /// Load items into fzf
    fn load<'a>(
        &'a self,
        state: &'a mut State,
        arg: Vec<String>,
    ) -> BoxFuture<'a, Result<LoadResp, String>>;

    /// Run command with the selected item
    fn preview<'a>(
        &'a self,
        state: &'a mut State,
        item: String,
    ) -> BoxFuture<'a, Result<PreviewResp, String>>;

    /// Run command with the selected item
    fn run<'a>(
        &'a self,
        state: &'a mut State,
        item: String,
        opts: RunOpts,
    ) -> BoxFuture<'a, Result<RunResp, String>>;
}

pub struct FzfConfig {
    pub myself: String,
    pub initial_query: Option<String>,
    pub socket: String,
    pub log_file: String,
    pub args: Vec<String>,
}

// TODO ここではないどこかへ
pub fn default_bindings() -> fzf::Bindings {
    use fzf::*;
    bindings! {
        "change" => vec![ first() ],
        "ctrl-s" => vec![ toggle_sort() ],
        "ctrl-o" => vec![ clear_query(), clear_screen() ],
        "ctrl-r" => vec![
            reload("reload"),
            clear_screen(),
        ],
        "pgdn" => vec![
            execute("change-mode menu"),
        ],
        "ctrl-f" => vec![
            execute("change-mode fd"),
        ],
        "ctrl-b" => vec![
            execute("change-mode buffer"),
        ],
        "ctrl-h" => vec![
            execute("change-mode mru"),
        ],
        "ctrl-g" => vec![
            execute("change-mode livegrep --query={q} -- --color=ansi {q}"),
        ],
        "alt-d" => vec![
            execute("change-mode zoxide"),
        ],
        "alt-w" => vec![
            execute("change-mode diagnostics"),
        ],
        "ctrl-i" => vec![
            execute("change-mode browser-history"),
        ],
        "ctrl-u" => vec![
            execute("change-directory --to-parent"),
            reload("reload"),
            clear_query(),
            clear_screen(),
        ],
        "ctrl-l" => vec![
            execute("change-directory --dir {}"),
            reload("reload"),
            clear_query(),
            clear_screen(),
        ],
        "ctrl-n" => vec![
            execute("change-directory --to-last-file-dir"),
            reload("reload"),
            clear_query(),
            clear_screen(),
        ],
        "enter" => vec![
            execute("run -- {}"),
            reload("reload"),
        ],
        "f1" => vec![
            execute("run -- {} --menu"),
        ],
        "ctrl-t" => vec![
            execute("run -- {} --tabedit"),
        ],
        "ctrl-v" => vec![
            execute("run -- {} --vifm"),
        ],
        "ctrl-d" => vec![
            execute("run -- {} --delete"),
            reload("reload"),
        ],
        "alt-g" => vec![
            execute("run -- {} --browse-github"),
        ],
    }
}
