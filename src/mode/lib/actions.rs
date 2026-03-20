use anyhow::Result;

use crate::env::Env;
use crate::nvim;
use crate::nvim::NeovimExt;
use crate::nvim::OpenTarget;
use crate::utils::gh;
use crate::utils::vscode;
use crate::utils::xsel;

pub async fn open_in_nvim(
    env: &Env,
    file: impl Into<OpenTarget>,
    line: Option<usize>,
    tabedit: bool,
) -> Result<()> {
    let nvim_opts = nvim::OpenOpts { line, tabedit };
    env.nvim.open(file.into(), nvim_opts).await?;
    Ok(())
}

pub async fn open_in_vscode(env: &Env, file: impl Into<String>, line: Option<usize>) -> Result<()> {
    let output = vscode::open(file.into(), line).await?;
    env.nvim.notify_command_result("code", output).await?;
    Ok(())
}

pub async fn yank(text: impl AsRef<str>) -> Result<()> {
    xsel::yank(text).await
}

pub async fn browse_github(file: impl AsRef<str>) -> Result<()> {
    gh::browse_github(file).await
}

pub async fn browse_github_line(
    file: impl AsRef<str>,
    revision: impl AsRef<str>,
    line: usize,
) -> Result<()> {
    gh::browse_github_line(file, revision, line).await
}

pub async fn oil(env: &Env) -> Result<()> {
    let cwd = std::env::current_dir().unwrap();
    env.nvim.hide_floaterm().await?;
    env.nvim
        .command(&format!("Oil --float {}", cwd.display()))
        .await?;
    Ok(())
}

pub async fn new_file(env: &Env, item: &str) -> Result<()> {
    use tokio::process::Command;
    let cwd = std::env::current_dir().unwrap();
    let fname = crate::utils::fzf::input_with_placeholder("Enter file name", item).await?;
    let fname = fname.trim();
    let path = format!("{}/{}", cwd.display(), fname);
    let dir = std::path::Path::new(&path).parent().unwrap();
    Command::new("mkdir").arg("-p").arg(dir).status().await?;
    Command::new("touch").arg(&path).status().await?;
    open_in_nvim(env, path, None, false).await
}

pub async fn execute_command(env: &Env, item: &str) -> Result<()> {
    let (cmd, output) = crate::utils::command::edit_and_run(format!(" {item}")).await?;
    env.nvim.notify_command_result(&cmd, output).await?;
    Ok(())
}
