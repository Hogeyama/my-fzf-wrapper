use tokio::process::Command;

pub async fn log_graph(commit: impl AsRef<str>) -> Result<Vec<String>, String> {
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
        .map_err(|e| e.to_string())?
        .stdout;
    Ok(String::from_utf8_lossy(commits.as_slice())
        .into_owned()
        .split('\n')
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

pub async fn reflog_graph(commit: impl AsRef<str>) -> Result<Vec<String>, String> {
    let commits = Command::new("git")
        .arg("reflog")
        .arg(
            "--pretty=format:%C(yellow)%h%Creset %C(green)%ad%Creset %s %Cred%d%Creset %Cblue[%an]",
        )
        .arg("--date=short")
        .arg("--color=always")
        .arg(commit.as_ref())
        .output()
        .await
        .map_err(|e| e.to_string())?
        .stdout;
    Ok(String::from_utf8_lossy(commits.as_slice())
        .into_owned()
        .split('\n')
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

pub async fn remote_branches() -> Result<Vec<String>, String> {
    let refs = Command::new("git")
        .arg("for-each-ref")
        .arg("--format=%(refname:short)")
        .arg("refs/remotes")
        .output()
        .await
        .map_err(|e| e.to_string())?
        .stdout;
    Ok(String::from_utf8_lossy(refs.as_slice())
        .into_owned()
        .split('\n')
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

pub async fn show_commit(commit: impl AsRef<str>) -> Result<String, String> {
    let format = [
        "%C(yellow)commit %H%Creset",
        "Author:       %aN <%aE>",
        "AuthorDate:   %ai",
        "Commiter:     %cN <%cE>",
        "CommiterDate: %ci",
        "Co-Author:    %(trailers:key=Co-authored-by)",
        "Refname:      %D",
        "",
        "%w(0,2,2)%B",
    ]
    .join("%n");
    let commit = Command::new("git")
        .arg("show")
        .arg("--color=always")
        .arg(format!("--format={format}"))
        .arg(commit.as_ref())
        .output()
        .await
        .map_err(|e| e.to_string())?
        .stdout;
    Ok(String::from_utf8_lossy(commit.as_slice()).into_owned())
}

pub async fn rev_parse(commit: impl AsRef<str>) -> Result<String, String> {
    let commit = Command::new("git")
        .arg("rev-parse")
        .arg(commit.as_ref())
        .output()
        .await
        .map_err(|e| e.to_string())?
        .stdout;
    Ok(String::from_utf8_lossy(commit.as_slice())
        .trim_end()
        .to_string())
}
