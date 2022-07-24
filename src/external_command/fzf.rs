use tokio::process::Command;

// binding は toml から読ませると良いのでは
pub fn new(myself: impl Into<String>, socket: impl Into<String>) -> Command {
    let myself = myself.into();
    let socket = socket.into();
    let mut fzf = Command::new("fzf");
    fzf.args(vec!["--bind", "ctrl-s:toggle-sort"]);
    // preview
    fzf.args(vec![
        "--preview",
        &format!("{myself} preview --socket {socket} {{}}"),
    ]);
    // fd: default
    fzf.args(vec![
        "--bind",
        &format!("ctrl-f:reload[{myself} load --socket {socket} -- fd]+change-prompt[files>]"),
    ]);
    // fd: cd-up
    fzf.args(vec![
        "--bind",
        &format!(
            "ctrl-u:reload[{myself} load --socket {socket} -- fd --cd-up]+change-prompt[files>]"
        ),
    ]);
    // fd: cd-arg
    fzf.args(vec![
        "--bind",
        &format!(
            "ctrl-l:reload[{myself} load --socket {socket} -- fd --cd {{}}]+change-prompt[files>]+clear-query"
        ),
    ]);
    // fd: cd-last-file
    fzf.args(vec![
        "--bind",
        &format!(
            "ctrl-n:reload[{myself} load --socket {socket} -- fd --cd-last-file]+change-prompt[files>]"
        ),
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
    fzf.args(vec!["--preview-window", "right:50%:noborder"]);
    fzf.args(vec!["--header-lines=1"]);
    fzf.args(vec!["--prompt", "files>"]);
    fzf.env(
        "FZF_DEFAULT_COMMAND",
        format!("{myself} load --socket {socket} -- fd"),
    );
    fzf
}

// reload("ctrl-r", `${prog} reload`, []),
// reload("ctrl-b", buffer.cmd.default, [prompt("buffer")]),
// reload("ctrl-h", mru.cmd.default, [prompt("file-history")]),
// reload("ctrl-d", zoxide.cmd.default, [prompt("dir-history")]),
// reload("ctrl-g", rg.cmd.default, [prompt("grep"), clQuery]),
// reload("ctrl-i", browser.cmd.default, [prompt("browser-history"), clQuery]),
// reload("alt-w", diagnostics.cmd.default, [prompt("diagnostics"), clQuery]),
