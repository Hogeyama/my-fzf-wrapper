use git2::{BranchType, Repository};
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

#[allow(dead_code)]
pub fn remotes() -> Result<Vec<String>, String> {
    let remotes = get_repo()?
        .remotes()
        .map_err(|e| e.to_string())?
        .iter()
        .filter_map(|r| r.map(|s| s.to_string()))
        .collect::<Vec<_>>();
    Ok(remotes)
}

#[allow(dead_code)]
pub fn local_branches() -> Result<Vec<String>, String> {
    list_branches(Some(BranchType::Local))
}

pub fn remote_branches() -> Result<Vec<String>, String> {
    list_branches(Some(BranchType::Remote))
}

pub fn rev_parse(commitish: impl AsRef<str>) -> Result<String, String> {
    Ok(get_repo()?
        .revparse_single(commitish.as_ref())
        .map_err(|e| e.to_string())?
        .id()
        .to_string())
}

fn list_branches(filter: Option<BranchType>) -> Result<Vec<String>, String> {
    let branches = get_repo()?
        .branches(filter)
        .map_err(|e| e.to_string())?
        .filter_map(|b| {
            b.ok()
                .and_then(|(b, _)| b.name().ok().flatten().map(|s| s.to_string()))
        })
        .collect::<Vec<_>>();
    Ok(branches)
}

fn get_repo() -> Result<Repository, String> {
    Repository::discover(".").map_err(|e| e.to_string())
}
