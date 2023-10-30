use git2::{BranchType, IntoCString, Repository, Status, StatusEntry, StatusOptions};
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

////////////////////////////////////////////////////////////////////////////////
// Status
////////////////////////////////////////////////////////////////////////////////

pub fn files_with_status(oneof: impl IntoIterator<Item = Status>) -> Result<Vec<String>, String> {
    let status_bits = oneof.into_iter().fold(Status::empty(), |acc, s| acc | s);
    Ok(get_repo()?
        .statuses(None)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter_map(|s| {
            if s.status().intersects(status_bits) {
                s.path().map(|s| s.to_string())
            } else {
                None
            }
        })
        .collect::<Vec<_>>())
}

#[allow(dead_code)]
pub fn status<F, T>(path: impl IntoCString, k: F) -> Result<T, String>
where
    F: FnOnce(StatusEntry<'_>) -> Result<T, String>,
{
    let repo = get_repo()?;
    let mut opts = StatusOptions::new();
    opts.pathspec(path);
    let statuses = repo.statuses(Some(&mut opts)).map_err(|e| e.to_string())?;
    let r = statuses.get(0).ok_or("no status")?;
    k(r)
}

////////////////////////////////////////////////////////////////////////////////
// Remote
////////////////////////////////////////////////////////////////////////////////

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

////////////////////////////////////////////////////////////////////////////////
// Branch
////////////////////////////////////////////////////////////////////////////////

#[allow(dead_code)]
pub fn local_branches() -> Result<Vec<String>, String> {
    list_branches(Some(BranchType::Local))
}

pub fn remote_branches() -> Result<Vec<String>, String> {
    list_branches(Some(BranchType::Remote))
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

////////////////////////////////////////////////////////////////////////////////
// Commit
////////////////////////////////////////////////////////////////////////////////

pub fn rev_parse(commitish: impl AsRef<str>) -> Result<String, String> {
    Ok(get_repo()?
        .revparse_single(commitish.as_ref())
        .map_err(|e| e.to_string())?
        .id()
        .to_string())
}

////////////////////////////////////////////////////////////////////////////////
// Repository
////////////////////////////////////////////////////////////////////////////////

pub fn get_repo() -> Result<Repository, String> {
    Repository::discover(".").map_err(|e| e.to_string())
}

pub fn workdir() -> Result<String, String> {
    Ok(get_repo()?
        .workdir()
        .ok_or("no workdir")?
        .to_str()
        .unwrap()
        .to_string())
}
