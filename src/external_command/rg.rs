use tokio::process::Command;

pub fn new() -> Command {
    let mut rg = Command::new("rg");
    rg.arg("--column");
    rg.arg("--line-number");
    rg.arg("--no-heading");
    rg.arg("--no-ignore-vcs");
    rg.arg("--hidden");
    rg.arg("--color=never");
    rg.arg("--smart-case");

    let extra_opts = std::env::var("FZFW_RG_EXTRA_OPTS");
    if let Ok(extra_opts) = extra_opts {
        // XXX オプションに,が含まれていると困る。が、多分ないはず
        for extra_opt in extra_opts.split(',') {
            rg.args(vec![extra_opt]);
        }
    }
    rg.kill_on_drop(true);
    rg
}
