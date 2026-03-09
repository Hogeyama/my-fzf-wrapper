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
