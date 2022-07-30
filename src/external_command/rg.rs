use tokio::process::Command;

pub fn new() -> Command {
    let mut rg = Command::new("rg");
    rg.arg("--column");
    rg.arg("--line-number");
    rg.arg("--no-heading");
    rg.arg("--color=never");
    rg.arg("--smart-case");
    rg
}
