use tokio::process::Command;

// binding は toml から読ませると良いのでは
pub fn new(myself: impl Into<String>, socket: impl Into<String>) -> Command {
    let myself = myself.into();
    let socket = socket.into();
    let mut fzf = Command::new("fzf");
    fzf.args(vec!["--ansi"]);
    fzf.args(vec!["--layout", "reverse"]);
    fzf.args(vec!["--bind", "ctrl-s:toggle-sort"]);
    fzf.args(vec!["--bind", "ctrl-o:clear-query+clear-screen"]);
    fzf.args(vec!["--bind", "change:first"]);
    // preview
    fzf.args(vec![
        "--preview",
        &format!("{myself} preview --socket {socket} {{}}"),
    ]);
    // reload
    fzf.args(vec![
        "--bind",
        &format!("ctrl-r:reload[{myself} reload --socket {socket}]+clear-screen"),
    ]);
    // fd: default
    fzf.args(vec![
        "--bind",
        &format!("ctrl-f:reload[{myself} load --socket {socket} -- fd]+change-prompt[files>]+clear-screen"),
    ]);
    // fd: cd-up
    fzf.args(vec![
        "--bind",
        &format!(
            "ctrl-u:reload[{myself} load --socket {socket} -- fd --cd-up]+change-prompt[files>]+clear-screen"
        ),
    ]);
    // fd: cd-arg
    fzf.args(vec![
        "--bind",
        &format!(
            "ctrl-l:reload[{myself} load --socket {socket} -- fd --cd {{}}]+change-prompt[files>]+clear-query+clear-screen"
        ),
    ]);
    // fd: cd-last-file
    fzf.args(vec![
        "--bind",
        &format!(
            "ctrl-n:reload[{myself} load --socket {socket} -- fd --cd-last-file]+change-prompt[files>]+clear-screen"
        ),
    ]);
    // rg: default
    fzf.args(vec![
        "--bind",
        &format!("ctrl-g:reload[{myself} load --socket {socket} -- rg {{q}}]+change-prompt[grep>]+clear-query+clear-screen"),
    ]);
    // buffer: default
    fzf.args(vec![
        "--bind",
        &format!("ctrl-b:reload[{myself} load --socket {socket} -- buffer]+change-prompt[buffer>]+clear-query+clear-screen"),
    ]);
    // mru: default
    fzf.args(vec![
        "--bind",
        &format!(
            "ctrl-h:reload[{myself} load --socket {socket} -- mru]+change-prompt[mru>]+clear-query+clear-screen"
        ),
    ]);
    // zoxide: default
    fzf.args(vec![
        "--bind",
        &format!("ctrl-d:reload[{myself} load --socket {socket} -- zoxide]+change-prompt[zoxide>]+clear-query+clear-screen"),
    ]);
    // diagnostics: default
    fzf.args(vec![
        "--bind",
        &format!("alt-w:reload[{myself} load --socket {socket} -- diagnostics]+change-prompt[diagnostics>]+clear-query+clear-screen"),
    ]);
    // browser-history: default
    fzf.args(vec![
        "--bind",
        &format!("ctrl-i:reload[{myself} load --socket {socket} -- browser-history]+change-prompt[browser>]+clear-query+clear-screen"),
    ]);
    // run: default
    fzf.args(vec![
        "--bind",
        &format!("enter:execute[{myself} run --socket {socket} -- {{}}]"),
    ]);
    // run: tabedit
    fzf.args(vec![
        "--bind",
        &format!("ctrl-t:execute[{myself} run --socket {socket} -- {{}} --tabedit]"),
    ]);
    // run: vifm
    fzf.args(vec![
        "--bind",
        &format!("ctrl-v:execute[{myself} run --socket {socket} -- {{}} --vifm]"),
    ]);
    fzf.args(vec!["--preview-window", "right:50%:noborder"]);
    fzf.args(vec!["--header-lines=1"]);
    fzf.args(vec!["--prompt", "files>"]);
    fzf.env(
        "FZF_DEFAULT_COMMAND",
        format!("{myself} load --socket {socket} -- fd"),
    );
    fzf
}
