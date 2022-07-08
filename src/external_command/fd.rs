use tokio::process::Command;

pub fn new() -> Command {
    let mut fd = Command::new("fd");
    fd.args(vec!["--hidden"]);
    fd.args(vec!["--no-ignore"]);
    fd.args(vec!["--type", "f"]);
    fd.args(vec!["--exclude", ".git"]);
    fd.args(vec!["--exclude", "target"]);
    fd.args(vec!["--exclude", "nvim-rs"]);
    // TODO 環境変数読む
    fd
}
