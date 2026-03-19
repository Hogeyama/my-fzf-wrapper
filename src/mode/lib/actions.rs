use anyhow::Result;

use crate::config::Config;
use crate::nvim;
use crate::nvim::NeovimExt;
use crate::nvim::OpenTarget;
use crate::utils::gh;
use crate::utils::vscode;
use crate::utils::xsel;

pub async fn open_in_nvim(
    config: &Config,
    file: impl Into<OpenTarget>,
    line: Option<usize>,
    tabedit: bool,
) -> Result<()> {
    let nvim_opts = nvim::OpenOpts { line, tabedit };
    config.nvim.open(file.into(), nvim_opts).await?;
    Ok(())
}

pub async fn open_in_vscode(
    config: &Config,
    file: impl Into<String>,
    line: Option<usize>,
) -> Result<()> {
    let output = vscode::open(file.into(), line).await?;
    config.nvim.notify_command_result("code", output).await?;
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

pub async fn oil(config: &Config) -> Result<()> {
    let cwd = std::env::current_dir().unwrap();
    config.nvim.hide_floaterm().await?;
    config
        .nvim
        .command(&format!("Oil --float {}", cwd.display()))
        .await?;
    Ok(())
}

pub async fn new_file(config: &Config, item: &str) -> Result<()> {
    use tokio::process::Command;
    let cwd = std::env::current_dir().unwrap();
    let fname = crate::utils::fzf::input_with_placeholder("Enter file name", item).await?;
    let fname = fname.trim();
    let path = format!("{}/{}", cwd.display(), fname);
    let dir = std::path::Path::new(&path).parent().unwrap();
    Command::new("mkdir").arg("-p").arg(dir).status().await?;
    Command::new("touch").arg(&path).status().await?;
    open_in_nvim(config, path, None, false).await
}

pub async fn execute_command(config: &Config, item: &str) -> Result<()> {
    let (cmd, output) = crate::utils::command::edit_and_run(format!(" {item}")).await?;
    config.nvim.notify_command_result(&cmd, output).await?;
    Ok(())
}
