use anyhow::Result;
use clap::Parser;
use futures::future::BoxFuture;
use futures::FutureExt;
use futures::StreamExt as _;
use once_cell::sync::Lazy;
use regex::Regex;

use super::lib::actions;
use super::lib::item::RegexItem;
use crate::env::Env;
use crate::logger::Serde;
use crate::method::LoadResp;
use crate::method::PreviewResp;
use crate::mode::config_builder;
use crate::mode::CallbackMap;
use crate::mode::ModeDef;
use crate::utils::bat;
use crate::utils::command;
use crate::utils::fzf;
use crate::utils::fzf::PreviewWindow;
use crate::utils::git;
use crate::utils::rg;

////////////////////////////////////////////////////////////////////////////////
// Livegrep
////////////////////////////////////////////////////////////////////////////////

#[derive(Clone)]
pub struct LiveGrep {
    name: &'static str,
    rg_opts: Vec<String>,
}

impl LiveGrep {
    pub fn new() -> Self {
        Self {
            name: "livegrep",
            rg_opts: vec!["--glob".to_string(), "!.git".to_string()],
        }
    }
    pub fn new_no_ignore() -> Self {
        Self {
            name: "livegrep(no-ignore)",
            rg_opts: vec!["--no-ignore".to_string()],
        }
    }
}

impl ModeDef for LiveGrep {
    fn name(&self) -> &'static str {
        self.name
    }
    fn load(&self, _env: &Env, query: String, _item: String) -> super::LoadStream {
        load(query, &self.rg_opts)
    }
    fn preview(
        &self,
        _env: &Env,
        _win: &PreviewWindow,
        item: String,
    ) -> BoxFuture<'static, Result<PreviewResp>> {
        async move { preview(item).await }.boxed()
    }
    fn fzf_bindings(&self) -> (super::ModeBindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= livegrep_common_bindings(),
            "change" => [
                b.reload(),
            ],
            "esc" => [
                b.change_mode(LiveGrepF.name(), false),
            ],
        }
    }
    fn mode_enter_actions(&self) -> Vec<fzf::Action> {
        vec![fzf::Action::DisableSearch]
    }
    fn wants_sort(&self) -> bool {
        false
    }
}

#[derive(Parser, Debug, Clone)]
pub struct LoadOpts {
    #[clap()]
    pub query: String,
}

fn load(query: String, opts: &Vec<String>) -> super::LoadStream {
    let mut rg_cmd = rg::new();
    rg_cmd.args(opts);
    rg_cmd.arg("--");
    rg_cmd.arg(query);
    Box::pin(async_stream::stream! {
        let stream = command::command_output_stream(rg_cmd).chunks(100); // tekito
        tokio::pin!(stream);
        let mut has_error = false;
        while let Some(r) = stream.next().await {
            let r = r.into_iter().collect::<Result<Vec<String>>>();
            match r {
                Ok(lines) => {
                    yield Ok(LoadResp::wip_with_default_header(lines));
                }
                Err(e) => {
                    yield Ok(LoadResp::error(e.to_string()));
                    has_error = true;
                    break;
                }
            }
        }
        if !has_error {
            yield Ok(LoadResp::last())
        }
    })
}

////////////////////////////////////////////////////////////////////////////////
// Fuzzy search after livegrep
////////////////////////////////////////////////////////////////////////////////

#[derive(Clone)]
pub struct LiveGrepF;

impl ModeDef for LiveGrepF {
    fn name(&self) -> &'static str {
        "livegrepf"
    }
    fn load<'a>(&'a self, env: &'a Env, _query: String, _item: String) -> super::LoadStream<'a> {
        Box::pin(async_stream::stream! {
            let livegrep_result = env.last_load_resp.lock().await.clone();
            let items = match livegrep_result {
                Some(resp) => resp.items,
                None => vec![],
            };
            yield Ok(LoadResp::new_with_default_header(items))
        })
    }
    fn preview(
        &self,
        _env: &Env,
        _win: &PreviewWindow,
        item: String,
    ) -> BoxFuture<'static, Result<PreviewResp>> {
        async move { preview(item).await }.boxed()
    }
    fn fzf_bindings(&self) -> (super::ModeBindings, CallbackMap) {
        livegrep_common_bindings()
    }
}

////////////////////////////////////////////////////////////////////////////////
// Common
////////////////////////////////////////////////////////////////////////////////

/// LiveGrep / LiveGrepF 共通のバインディング
fn livegrep_common_bindings() -> (super::ModeBindings, super::CallbackMap) {
    use config_builder::*;
    bindings! {
        b <= default_bindings(),
        "enter" => [ b.open_nvim(LIVEGREP_ITEM, false) ],
        "ctrl-space" => [ b.open_vscode(LIVEGREP_ITEM) ],
        "ctrl-t" => [ b.open_nvim(LIVEGREP_ITEM, true) ],
        "pgup" => [
            b.execute(|_mode,env,_query,item| async move {
                match &*fzf::select(vec!["browse-github", "neovim", "vscode"]).await? {
                    "browse-github" => {
                        let file = ITEM_PATTERN.replace(&item, "$file").into_owned();
                        let line = ITEM_PATTERN.replace(&item, "$line").into_owned();
                        let revision = git::rev_parse("HEAD")?;
                        actions::browse_github_line(file, &revision, line.parse::<usize>().unwrap()).await
                    },
                    "neovim" => {
                        let file = ITEM_PATTERN.replace(&item, "$file").into_owned();
                        let line = ITEM_PATTERN.replace(&item, "$line").into_owned();
                        actions::open_in_nvim(env, file, line.parse().ok(), false).await
                    },
                    "vscode" => {
                        let file = ITEM_PATTERN.replace(&item, "$file").into_owned();
                        let line = ITEM_PATTERN.replace(&item, "$line").into_owned();
                        actions::open_in_vscode(env, file, line.parse().ok()).await
                    },
                    _ => Ok(()),
                }
            }.boxed())
        ]
    }
}

static ITEM_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?P<file>[^:]*):(?P<line>\d+):(?P<col>\d+):.*").unwrap());

static LIVEGREP_ITEM: RegexItem = RegexItem {
    pattern: &ITEM_PATTERN,
    file_group: "$file",
    line_group: Some("$line"),
};

async fn preview(item: String) -> Result<PreviewResp> {
    let file = ITEM_PATTERN.replace(&item, "$file").into_owned();
    let line = ITEM_PATTERN.replace(&item, "$line").into_owned();
    let col = ITEM_PATTERN.replace(&item, "$col").into_owned();
    match line.parse::<isize>() {
        Ok(line) => {
            info!("rg.preview"; "parsed" => Serde(json!({
                "file": file,
                "line": line,
                "col": col
            })));
            let message = bat::render_file_with_highlight(&file, line).await?;
            Ok(PreviewResp { message })
        }
        Err(e) => {
            error!("rg: preview: parse line failed"; "error" => e.to_string(), "line" => line);
            Ok(PreviewResp {
                message: "".to_string(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn livegrep_name() {
        assert_eq!(LiveGrep::new().name(), "livegrep");
    }

    #[test]
    fn livegrep_no_ignore_name() {
        assert_eq!(LiveGrep::new_no_ignore().name(), "livegrep(no-ignore)");
    }

    #[test]
    fn livegrepf_name() {
        assert_eq!(LiveGrepF.name(), "livegrepf");
    }

    #[test]
    fn item_pattern_parses_rg_output() {
        let item = "src/main.rs:10:5:fn main() {";
        let file = ITEM_PATTERN.replace(item, "$file").into_owned();
        let line = ITEM_PATTERN.replace(item, "$line").into_owned();
        let col = ITEM_PATTERN.replace(item, "$col").into_owned();
        assert_eq!(file, "src/main.rs");
        assert_eq!(line, "10");
        assert_eq!(col, "5");
    }

    #[test]
    fn item_pattern_with_colon_in_content() {
        let item = "config.rs:1:1:use std::collections::HashMap;";
        let file = ITEM_PATTERN.replace(item, "$file").into_owned();
        let line = ITEM_PATTERN.replace(item, "$line").into_owned();
        assert_eq!(file, "config.rs");
        assert_eq!(line, "1");
    }

    #[test]
    fn livegrep_item_extractor_file() {
        use super::super::lib::item::ItemExtractor;
        assert_eq!(
            LIVEGREP_ITEM.file("src/lib.rs:42:1:pub mod foo;").unwrap(),
            "src/lib.rs"
        );
    }

    #[test]
    fn livegrep_item_extractor_line() {
        use super::super::lib::item::ItemExtractor;
        assert_eq!(LIVEGREP_ITEM.line("src/lib.rs:42:1:pub mod foo;"), Some(42));
    }

    #[test]
    fn livegrep_wants_sort_false() {
        assert!(!LiveGrep::new().wants_sort());
    }

    #[test]
    fn livegrep_mode_enter_disables_search() {
        let actions = LiveGrep::new().mode_enter_actions();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].render(), "disable-search");
    }
}
