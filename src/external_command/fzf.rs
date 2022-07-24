use tokio::process::Command;

// binding は toml から読ませると良いのでは
pub fn new(myself: impl Into<String>, socket: impl Into<String>) -> Command {
    let myself = myself.into();
    let socket = socket.into();
    let mut fzf = Command::new("fzf");
    fzf.args(vec!["--bind", "ctrl-s:toggle-sort"]);
    fzf.args(vec![
        "--preview",
        &format!("{myself} preview --socket {socket} {{}}"),
    ]);
    fzf.args(vec![
        "--bind",
        &format!(
            "ctrl-f:reload[{myself} load --socket {socket} -- fd {{q}}]+change-prompt[files>]"
        ),
    ]);
    fzf.args(vec![
        "--bind",
        &format!("enter:execute[{myself} run --socket {socket} -- {{}}]"),
    ]);
    fzf.args(vec!["--preview-window", "right:50%:noborder"]);
    fzf.args(vec!["--header-lines=1"]);
    fzf.args(vec!["--prompt", "files>"]);
    fzf.env(
        "FZF_DEFAULT_COMMAND",
        format!("{myself} load --socket {socket} -- fd ."),
    );
    fzf
}

// exec("ctrl-o", nvimR.cmd.default, []),
// exec("ctrl-t", nvimR.cmd.tabEdit, []),
// exec("ctrl-v", vifmR.cmd.default, []),
// reload("ctrl-r", `${prog} reload`, []),
// reload("ctrl-f", fd.cmd.default, [prompt("files")]),
// reload("ctrl-u", fd.cmd.cdUp, [prompt("files")]),
// reload("ctrl-l", fd.cmd.cdArg, [prompt("files"), clQuery]),
// reload("ctrl-n", fd.cmd.cdLastFile, [prompt("files"), clQuery]),
// reload("ctrl-b", buffer.cmd.default, [prompt("buffer")]),
// reload("ctrl-h", mru.cmd.default, [prompt("file-history")]),
// reload("ctrl-d", zoxide.cmd.default, [prompt("dir-history")]),
// reload("ctrl-g", rg.cmd.default, [prompt("grep"), clQuery]),
// reload("ctrl-i", browser.cmd.default, [prompt("browser-history"), clQuery]),
// reload("alt-w", diagnostics.cmd.default, [prompt("diagnostics"), clQuery]),
