#![allow(dead_code)]
use std::collections::HashMap;

use tokio::{io::AsyncWriteExt, process::Command};

pub struct Config {
    pub myself: String,
    pub socket: String,
    pub log_file: String,
    pub load: Vec<String>,
    pub initial_prompt: String,
    pub initial_query: Option<String>,
    pub bindings: Bindings,
    pub extra_opts: Vec<String>,
}

type Key = String;

pub struct Bindings(pub HashMap<Key, Vec<Action>>);

impl Bindings {
    pub fn merge(mut self, other: Self) -> Self {
        self.0.extend(other.0);
        self
    }
}

#[macro_export]
macro_rules! bindings {
    ($($k:expr => $v:expr),* $(,)?) => {{
        Bindings(core::convert::From::from([$(($k.to_string(), $v),)*]))
    }};
}
pub use bindings;

pub enum Action {
    Reload(String),
    Execute(String),
    ChangePrompt(String),
    ToggleSort,
    ClearQuery,
    ClearScreen,
    First,
}

pub fn reload(cmd: impl AsRef<str>) -> Action {
    Action::Reload(cmd.as_ref().to_string())
}

pub fn execute(cmd: impl Into<String>) -> Action {
    Action::Execute(cmd.into())
}

pub fn change_prompt(prompt: impl Into<String>) -> Action {
    Action::ChangePrompt(prompt.into())
}

pub fn toggle_sort() -> Action {
    Action::ToggleSort
}

pub fn clear_query() -> Action {
    Action::ClearQuery
}

pub fn clear_screen() -> Action {
    Action::ClearScreen
}

pub fn first() -> Action {
    Action::First
}

impl Action {
    fn render(&self, myself: &str) -> String {
        match self {
            Action::Reload(cmd) => format!("reload[{myself} {cmd}]"),
            Action::Execute(cmd) => format!("execute[{myself} {cmd}]"),
            Action::ChangePrompt(prompt) => format!("change-prompt[{prompt}]"),
            Action::ToggleSort => "toggle-sort".to_string(),
            Action::ClearQuery => "clear-query".to_string(),
            Action::ClearScreen => "clear-screen".to_string(),
            Action::First => "first".to_string(),
        }
    }
}

pub fn new(config: Config) -> Command {
    let Config {
        myself,
        socket,
        log_file,
        load,
        initial_prompt,
        initial_query,
        bindings,
        extra_opts,
    } = config;
    let mut fzf = Command::new("fzf");
    fzf.kill_on_drop(true);

    // Envirionment variables
    fzf.env(
        "FZF_DEFAULT_COMMAND",
        shellwords::join(
            &[
                vec![myself.as_ref(), "load"],
                load.iter().map(|s| s.as_ref()).collect::<Vec<_>>(),
            ]
            .concat(),
        ),
    );
    fzf.env("FZFW_LOG_FILE", log_file);
    fzf.env("FZFW_SOCKET", socket);

    let c = |s: &str| s.to_string();

    #[rustfmt::skip]
    let mut args = vec![
        c("--ansi"),
        c("--header-lines"), c("1"),
        c("--layout"), c("reverse"),
        c("--query"), initial_query.unwrap_or_default(),
        c("--preview"), format!("{myself} preview {{}}"),
        c("--preview-window"), c("right:50%:noborder"),
        c("--prompt"), initial_prompt
    ];

    bindings.0.iter().for_each(|(key, actions)| {
        let actions = actions
            .iter()
            .map(|action| action.render(&myself))
            .collect::<Vec<_>>();
        args.push("--bind".to_string());
        args.push(format!("{}:{}", key, actions.join("+")));
    });

    extra_opts.iter().for_each(|opt| {
        args.push(opt.to_string());
    });

    fzf.args(args);

    fzf
}

pub fn new_livegrep(myself: impl Into<String>, socket: impl Into<String>) -> Command {
    let myself = myself.into();
    let socket = socket.into();
    let mut fzf = Command::new("fzf");
    fzf.args(vec!["--ansi"]);
    fzf.args(vec!["--layout", "reverse"]);
    fzf.args(vec!["--bind", "ctrl-s:toggle-sort"]);
    fzf.args(vec!["--bind", "ctrl-o:clear-query+clear-screen"]);
    // Disable fuzzy search
    fzf.args(vec!["--disabled"]);
    // livegrep
    fzf.args(vec![
        "--bind",
        &format!("change:reload[{myself} live-grep update -- {{q}}]"),
    ]);
    // preview
    fzf.args(vec!["--preview", &format!("{myself} preview {{}}")]);
    // run: default
    fzf.args(vec![
        "--bind",
        &format!("enter:execute[{myself} run -- {{}}]"),
    ]);
    // run: menu
    fzf.args(vec![
        "--bind",
        &format!("f1:execute[{myself} run -- {{}} --menu]"),
    ]);
    // run: browse-github
    fzf.args(vec![
        "--bind",
        &format!("alt-g:execute[{myself} run -- {{}} --browse-github]"),
    ]);
    fzf.args(vec!["--preview-window", "right:50%:noborder"]);
    fzf.args(vec!["--header-lines=1"]);
    fzf.args(vec!["--prompt", "livegrep>"]);
    fzf.env("FZF_DEFAULT_COMMAND", format!("echo -n"));
    fzf.env("FZFW_LOG_FILE", format!("/tmp/fzfw-livegrep.log"));
    fzf.env("FZFW_SOCKET", socket);
    fzf.kill_on_drop(true);
    fzf
}

pub async fn select(items: Vec<&str>) -> Result<String, String> {
    let mut fzf = Command::new("fzf")
        .arg("--ansi")
        .args(vec!["--layout", "reverse"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;

    let mut stdin = fzf.stdin.take().unwrap();
    stdin.write_all(items.join("\n").as_bytes()).await.unwrap();
    drop(stdin);

    Ok(String::from_utf8_lossy(
        &fzf.wait_with_output()
            .await
            .map_err(|e| e.to_string())?
            .stdout,
    )
    .trim()
    .to_string())
}
