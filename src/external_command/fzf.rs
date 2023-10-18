use tokio::{io::AsyncWriteExt, process::Command};

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
    fzf.args(vec!["--preview", &format!("{myself} preview {{}}")]);
    // reload
    fzf.args(vec![
        "--bind",
        &format!("ctrl-r:reload[{myself} reload]+clear-screen"),
    ]);
    // fd: default
    fzf.args(vec![
        "--bind",
        &format!("ctrl-f:reload[{myself} load -- fd]+change-prompt[files>]+clear-screen"),
    ]);
    // fd: cd-up
    fzf.args(vec![
        "--bind",
        &format!("ctrl-u:reload[{myself} load -- fd --cd-up]+change-prompt[files>]+clear-screen"),
    ]);
    // fd: cd-arg
    fzf.args(vec![
        "--bind",
        &format!(
            "ctrl-l:reload[{myself} load -- fd --cd {{}}]+change-prompt[files>]+clear-query+clear-screen"
        ),
    ]);
    // fd: cd-last-file
    fzf.args(vec![
        "--bind",
        &format!(
            "ctrl-n:reload[{myself} load -- fd --cd-last-file]+change-prompt[files>]+clear-screen"
        ),
    ]);
    // rg: default
    fzf.args(vec![
        "--bind",
        &format!("ctrl-space:reload[{myself} load -- rg {{q}}]+change-prompt[grep>]+clear-query+clear-screen"),
    ]);
    // buffer: default
    fzf.args(vec![
        "--bind",
        &format!("ctrl-b:reload[{myself} load -- buffer]+change-prompt[buffer>]+clear-query+clear-screen"),
    ]);
    // mru: default
    fzf.args(vec![
        "--bind",
        &format!(
            "ctrl-h:reload[{myself} load -- mru]+change-prompt[mru>]+clear-query+clear-screen"
        ),
    ]);
    // zoxide: default
    fzf.args(vec![
        "--bind",
        &format!(
            "alt-d:reload[{myself} load -- zoxide]+change-prompt[zoxide>]+clear-query+clear-screen"
        ),
    ]);
    // diagnostics: default
    fzf.args(vec![
        "--bind",
        &format!("alt-w:reload[{myself} load -- diagnostics]+change-prompt[diagnostics>]+clear-query+clear-screen"),
    ]);
    // browser-history: default
    fzf.args(vec![
        "--bind",
        &format!("ctrl-i:reload[{myself} load -- browser-history]+change-prompt[browser>]+clear-query+clear-screen"),
    ]);
    // run: default
    fzf.args(vec![
        "--bind",
        &format!("enter:execute[{myself} run -- {{}}]"),
    ]);
    // run: tabedit
    fzf.args(vec![
        "--bind",
        &format!("ctrl-t:execute[{myself} run -- {{}} --tabedit]"),
    ]);
    // run: vifm
    fzf.args(vec![
        "--bind",
        &format!("ctrl-v:execute[{myself} run -- {{}} --vifm]"),
    ]);
    // run: delete
    fzf.args(vec![
        "--bind",
        &format!("ctrl-d:execute[{myself} run -- {{}} --delete]+reload[{myself} reload]"),
    ]);
    // run: delete
    fzf.args(vec![
        "--bind",
        &format!("ctrl-d:execute[{myself} run -- {{}} --delete]+reload[{myself} reload]"),
    ]);
    // run: browse-github
    // TODO run はメニューを表示して選べるようにするのがいいかなあ
    fzf.args(vec![
        "--bind",
        &format!("alt-g:execute[{myself} run -- {{}} --browse-github]"),
    ]);
    fzf.args(vec![
        "--bind",
        &format!("f1:execute[{myself} run -- {{}} --menu]"),
    ]);
    // livegrep
    fzf.args(vec![
        "--bind",
        &format!(
            "ctrl-g:execute[{myself} live-grep start]+reload[{myself} live-grep get-result]+change-prompt[livegrep(fuzzy)>]+clear-query+clear-screen"
        ),
    ]);
    fzf.args(vec!["--preview-window", "right:50%:noborder"]);
    fzf.args(vec!["--header-lines=1"]);
    fzf.args(vec!["--prompt", "files>"]);
    fzf.env("FZF_DEFAULT_COMMAND", format!("{myself} load -- fd"));
    fzf.env("FZFW_SOCKET", socket);
    fzf.kill_on_drop(true);
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

pub async fn select(items: Vec<&str>) -> String {
    let mut fzf = Command::new("fzf")
        .args(vec!["--layout", "reverse"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    let mut stdin = fzf.stdin.take().unwrap();
    stdin.write_all(items.join("\n").as_bytes()).await.unwrap();
    drop(stdin);

    String::from_utf8_lossy(&fzf.wait_with_output().await.unwrap().stdout)
        .trim()
        .to_string()
}
