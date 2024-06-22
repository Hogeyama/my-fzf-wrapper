use std::process::Output;

use anyhow::anyhow;
use anyhow::Result;
use git2::BranchType;
use git2::IntoCString;
use git2::Repository;
use git2::Status;
use git2::StatusEntry;
use git2::StatusOptions;
use regex::Regex;
use tokio::process::Command;

use crate::utils::fzf;

////////////////////////////////////////////////////////////////////////////////
// Log
////////////////////////////////////////////////////////////////////////////////

pub async fn log_graph(commit: impl AsRef<str>) -> Result<Vec<String>> {
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
        .await?
        .stdout;
    Ok(String::from_utf8_lossy(commits.as_slice())
        .split('\n')
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

pub async fn reflog_graph(commit: impl AsRef<str>) -> Result<Vec<String>> {
    let commits = Command::new("git")
        .arg("reflog")
        .arg(
            "--pretty=format:%C(yellow)%h%Creset %C(green)%ad%Creset %s %Cred%d%Creset %Cblue[%an]",
        )
        .arg("--date=short")
        .arg("--color=always")
        .arg(commit.as_ref())
        .output()
        .await?
        .stdout;
    Ok(String::from_utf8_lossy(commits.as_slice())
        .split('\n')
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

// log_graph の %d [%an] 部分をパースする
pub fn parse_branches_of_log(line: impl AsRef<str>) -> Vec<String> {
    Regex::new(r"\(([^()]+)\) \[.*\]$")
        .unwrap()
        .captures(line.as_ref())
        .map(|c| c.get(1).unwrap().as_str().to_string())
        .unwrap_or("".to_string())
        .split(", ")
        .map(|s| s.strip_prefix("HEAD -> ").unwrap_or(s).to_string())
        .filter(|s| s.is_empty() || !s.starts_with("tag: "))
        .collect::<Vec<_>>()
}

////////////////////////////////////////////////////////////////////////////////
// Commit
////////////////////////////////////////////////////////////////////////////////

pub async fn show_commit(commit: impl AsRef<str>) -> Result<String> {
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
        .await?
        .stdout;
    Ok(String::from_utf8_lossy(commit.as_slice()).into_owned())
}

pub fn parse_short_commit(commit: impl AsRef<str>) -> Result<String> {
    Regex::new(r"[0-9a-f]{7}")
        .unwrap()
        .find(commit.as_ref())
        .map(|m| m.as_str().to_string())
        .ok_or(anyhow!("no commit found"))
}

pub async fn select_commit(context: impl AsRef<str>) -> Result<String> {
    let commits = log_graph("HEAD").await?;
    let commits = commits.iter().map(|s| s.as_str()).collect();
    let commit_line = fzf::select_with_header(context, commits).await?;
    parse_short_commit(commit_line)
}

////////////////////////////////////////////////////////////////////////////////
// Diff
////////////////////////////////////////////////////////////////////////////////

#[allow(dead_code)]
pub async fn diff() -> Result<String> {
    let diff = Command::new("git")
        .arg("diff")
        .arg("--no-ext")
        .output()
        .await?
        .stdout;
    Ok(String::from_utf8_lossy(diff.as_slice()).into_owned())
}

#[allow(dead_code)]
pub async fn diff_cached() -> Result<String> {
    let diff = Command::new("git")
        .arg("diff")
        .arg("--no-ext")
        .arg("--cached")
        .output()
        .await?
        .stdout;
    Ok(String::from_utf8_lossy(diff.as_slice()).into_owned())
}

pub async fn apply(patch_file: String, args: Vec<&str>) -> Result<Output> {
    let r = Command::new("git")
        .current_dir(workdir()?)
        .arg("apply")
        .args(args)
        .arg(&patch_file)
        .output()
        .await?;
    Ok(r)
}

////////////////////////////////////////////////////////////////////////////////
// Status
////////////////////////////////////////////////////////////////////////////////

pub fn files_with_status(oneof: impl IntoIterator<Item = Status>) -> Result<Vec<String>> {
    let status_bits = oneof.into_iter().fold(Status::empty(), |acc, s| acc | s);
    Ok(get_repo()?
        .statuses(None)?
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
pub fn status<F, T>(path: impl IntoCString, k: F) -> Result<T>
where
    F: FnOnce(StatusEntry<'_>) -> Result<T>,
{
    let repo = get_repo()?;
    let mut opts = StatusOptions::new();
    opts.pathspec(path);
    let statuses = repo.statuses(Some(&mut opts))?;
    let r = statuses.get(0).ok_or(anyhow!("no status"))?;
    k(r)
}

pub fn untracked_files() -> Result<Vec<String>> {
    files_with_status([Status::WT_NEW])
}

pub fn index_new_files() -> Result<Vec<String>> {
    files_with_status([Status::INDEX_NEW])
}

pub fn workingtree_modified_files() -> Result<Vec<String>> {
    files_with_status([Status::WT_MODIFIED])
}

pub fn index_modified_files() -> Result<Vec<String>> {
    files_with_status([Status::INDEX_MODIFIED])
}

pub fn workingtree_deleted_files() -> Result<Vec<String>> {
    files_with_status([Status::WT_DELETED])
}

pub fn index_deleted_files() -> Result<Vec<String>> {
    files_with_status([Status::INDEX_DELETED])
}

pub fn conflicted_files() -> Result<Vec<String>> {
    files_with_status([Status::CONFLICTED])
}

pub async fn stage_file(file: impl AsRef<str>) -> Result<Output> {
    let output = Command::new("git")
        .current_dir(workdir()?)
        .arg("add")
        .arg("--")
        .arg(file.as_ref())
        .output()
        .await?;
    Ok(output)
}

pub async fn unstage_file(file: impl AsRef<str>) -> Result<Output> {
    let output = Command::new("git")
        .current_dir(workdir()?)
        .arg("reset")
        .arg("--")
        .arg(file.as_ref())
        .output()
        .await?;
    Ok(output)
}

pub async fn restore_file(
    file: impl AsRef<str>,
    source: Option<impl AsRef<str>>,
) -> Result<Output> {
    let mut cmd = Command::new("git");
    cmd.current_dir(workdir()?).arg("restore");
    if let Some(source) = source {
        cmd.arg("--source").arg(source.as_ref());
    }
    cmd.arg(file.as_ref());
    let output = cmd.output().await?;
    Ok(output)
}

////////////////////////////////////////////////////////////////////////////////
// Remote
////////////////////////////////////////////////////////////////////////////////

pub fn remotes() -> Result<Vec<String>> {
    let remotes = get_repo()?
        .remotes()?
        .iter()
        .filter_map(|r| r.map(|s| s.to_string()))
        .collect::<Vec<_>>();
    Ok(remotes)
}

pub async fn push(
    remote: impl AsRef<str>,
    from: impl AsRef<str>,
    to: impl AsRef<str>,
    force: bool,
) -> Result<Output> {
    let output = Command::new("git")
        .arg("push")
        .args(if force { vec!["-f"] } else { vec![] })
        .arg(remote.as_ref())
        .arg(format!("{}:{}", from.as_ref(), to.as_ref()))
        .output()
        .await?;
    Ok(output)
}

////////////////////////////////////////////////////////////////////////////////
// Branch
////////////////////////////////////////////////////////////////////////////////

pub fn head() -> Result<String> {
    let head = get_repo()?
        .head()?
        .name()
        .ok_or(anyhow!("no head"))?
        .strip_prefix("refs/heads/")
        .ok_or(anyhow!("no head"))?
        .to_string();
    Ok(head)
}

pub fn upstream_of(branch: impl AsRef<str>) -> Result<String> {
    let repo = get_repo()?;
    let branch = repo.find_branch(branch.as_ref(), BranchType::Local)?;
    let upstream = branch.upstream()?;
    Ok(upstream.name()?.ok_or(anyhow!("no upstream"))?.to_string())
}

pub fn local_branches() -> Result<Vec<String>> {
    list_branches(Some(BranchType::Local))
}

pub fn remote_branches() -> Result<Vec<String>> {
    Ok(list_branches(Some(BranchType::Remote))?
        .into_iter()
        .filter(|b| !b.ends_with("/HEAD"))
        .collect::<Vec<_>>())
}

fn list_branches(filter: Option<BranchType>) -> Result<Vec<String>> {
    let branches = get_repo()?
        .branches(filter)?
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

pub fn rev_parse(commitish: impl AsRef<str>) -> Result<String> {
    Ok(get_repo()?
        .revparse_single(commitish.as_ref())?
        .id()
        .to_string())
}

////////////////////////////////////////////////////////////////////////////////
// Repository
////////////////////////////////////////////////////////////////////////////////

pub fn get_repo() -> Result<Repository> {
    Ok(Repository::discover(".")?)
}

pub fn workdir() -> Result<String> {
    Ok(get_repo()?
        .workdir()
        .ok_or(anyhow!("no workdir"))?
        .to_str()
        .unwrap()
        .to_string())
}
