use tokio::process::Command;

pub fn new() -> Command {
    let mut zoxide = Command::new("zoxide");
    zoxide.arg("query");
    zoxide.arg("--list");
    zoxide.kill_on_drop(true);
    zoxide
}
