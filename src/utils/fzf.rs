#![allow(dead_code)]
use std::collections::HashMap;

use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

// TODO 多くを mode/mod.rs に移動させる。myself を知っているのはおかしい

pub struct Config {
    pub myself: String,
    pub socket: String,
    pub log_file: String,
    pub load: Vec<String>,
    pub initial_prompt: String,
    pub initial_query: String,
    pub bindings: Bindings,
    pub extra_opts: Vec<String>,
}

pub type Key = String;

pub struct Bindings(pub HashMap<Key, Vec<Action>>);

impl Bindings {
    pub fn empty() -> Self {
        Bindings(HashMap::new())
    }
    pub fn merge(mut self, other: Self) -> Self {
        self.0.extend(other.0);
        self
    }
}

pub enum Action {
    Reload(String),
    Execute(String),
    ExecuteSilent(String),
    ChangePrompt(String),
    ToggleSort,
    ClearQuery,
    ClearScreen,
    First,
    Toggle,
    Raw(String),
}

impl Action {
    fn render(&self, myself: &str) -> String {
        match self {
            Action::Reload(cmd) => format!("reload[{myself} {cmd}]"),
            Action::Execute(cmd) => format!("execute[{myself} {cmd}]"),
            Action::ExecuteSilent(cmd) => format!("execute-silent[{myself} {cmd}]"),
            Action::ChangePrompt(prompt) => format!("change-prompt[{prompt}]"),
            Action::ToggleSort => "toggle-sort".to_string(),
            Action::ClearQuery => "clear-query".to_string(),
            Action::ClearScreen => "clear-screen".to_string(),
            Action::First => "first".to_string(),
            Action::Toggle => "toggle".to_string(),
            Action::Raw(s) => s.to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct PreviewWindow {
    pub lines: usize,
    pub columns: usize,
}

impl PreviewWindow {
    pub fn from_env() -> Option<Self> {
        let lines = std::env::var("FZF_PREVIEW_LINES").ok()?.parse().ok()?;
        let columns = std::env::var("FZF_PREVIEW_COLUMNS").ok()?.parse().ok()?;
        Some(Self { lines, columns })
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
                vec![myself.as_ref()],
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
        c("--no-sort"),
        c("--header-lines"), c("1"),
        c("--layout"), c("reverse"),
        c("--query"), initial_query,
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

pub async fn select(items: Vec<&str>) -> Result<String> {
    let mut fzf = Command::new("fzf")
        .arg("--ansi")
        .arg("--no-sort")
        .args(vec!["--layout", "reverse"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()?;

    let mut stdin = fzf.stdin.take().unwrap();
    stdin.write_all(items.join("\n").as_bytes()).await.unwrap();
    drop(stdin);

    Ok(
        String::from_utf8_lossy(&fzf.wait_with_output().await?.stdout)
            .trim()
            .to_string(),
    )
}

pub async fn select_with_header(header: impl AsRef<str>, items: Vec<&str>) -> Result<String> {
    let mut fzf = Command::new("fzf")
        .arg("--ansi")
        .arg("--no-sort")
        .args(vec!["--header-lines", "1"])
        .args(vec!["--layout", "reverse"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()?;

    let mut stdin = fzf.stdin.take().unwrap();
    let header = format!("{}\n", header.as_ref());
    stdin.write_all(header.as_bytes()).await.unwrap();
    stdin.write_all(items.join("\n").as_bytes()).await.unwrap();
    drop(stdin);

    Ok(
        String::from_utf8_lossy(&fzf.wait_with_output().await?.stdout)
            .trim()
            .to_string(),
    )
}

pub async fn input(header: impl AsRef<str>) -> Result<String> {
    let fzf = Command::new("fzf")
        .arg("--ansi")
        .args(vec!["--header", header.as_ref()])
        .args(vec!["--layout", "reverse"])
        .args(vec!["--bind", "enter:print-query"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .spawn()?;

    Ok(
        String::from_utf8_lossy(&fzf.wait_with_output().await?.stdout)
            .trim()
            .to_string(),
    )
}
