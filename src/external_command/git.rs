use tokio::process::Command;

pub async fn log_graph(commit: impl AsRef<str>) -> Vec<String> {
    let commits = Command::new("git")
        .arg("log")
        .arg(
            "--pretty=format:%C(yellow)%h%Creset %C(green)%ad%Creset %s %Cred%d%Creset %Cblue[%an]",
        )
        .arg("--date=short")
        .arg("--graph")
        .arg("--color=always")
        .arg(commit.as_ref())
        .output()
        .await
        .map_err(|e| e.to_string())
        .unwrap()
        .stdout;
    String::from_utf8_lossy(commits.as_slice())
        .into_owned()
        .split('\n')
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub async fn remote_branches() -> Vec<String> {
    let refs = Command::new("git")
        .arg("for-each-ref")
        .arg("--format=%(refname:short)")
        .arg("refs/remotes")
        .output()
        .await
        .map_err(|e| e.to_string())
        .unwrap()
        .stdout;
    String::from_utf8_lossy(refs.as_slice())
        .into_owned()
        .split('\n')
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty() && s.contains("/"))
        .collect()
}

pub async fn show_commit(commit: impl AsRef<str>) -> String {
    let commit = Command::new("git")
        .arg("show")
        .arg("--color=always")
        .arg(commit.as_ref())
        .output()
        .await
        .map_err(|e| e.to_string())
        .unwrap()
        .stdout;
    String::from_utf8_lossy(commit.as_slice()).into_owned()
}
