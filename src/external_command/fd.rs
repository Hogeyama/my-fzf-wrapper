use tokio::process::Command;

pub fn new() -> Command {
    let mut fd = Command::new("fd");
    fd.args(vec!["--hidden"]);
    fd.args(vec!["--follow"]);
    fd.args(vec!["--no-ignore"]);
    fd.args(vec!["--type", "f"]);
    fd.args(vec!["--exclude", ".git"]);

    let exclude_paths = std::env::var("FZFW_FD_EXCLUDE_PATHS");
    if let Ok(exclude_paths) = exclude_paths {
        for exclude_path in exclude_paths.split(',') {
            fd.args(vec!["--exclude", exclude_path]);
        }
    }

    let extra_opts = std::env::var("FZFW_FD_EXTRA_OPTS");
    if let Ok(extra_opts) = extra_opts {
        // XXX オプションに,が含まれていると困る。が、多分ないはず
        for extra_opt in extra_opts.split(',') {
            fd.args(vec![extra_opt]);
        }
    }
    fd.kill_on_drop(true);
    fd
}
