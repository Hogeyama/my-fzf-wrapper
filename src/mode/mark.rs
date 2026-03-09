use std::collections::HashMap;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use futures::future::BoxFuture;
use futures::FutureExt;
use once_cell::sync::Lazy;
use regex::Regex;
use rmpv::ext::from_value;
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::Mutex;

use crate::config::Config;
use crate::logger::Serde;
use crate::method::LoadResp;
use crate::method::PreviewResp;
use super::lib::actions;
use crate::mode::config_builder;
use crate::mode::CallbackMap;
use crate::mode::ModeDef;
use crate::nvim::Neovim;
use crate::nvim::NeovimExt;
use crate::state::State;
use crate::utils::bat;
use crate::utils::fzf;
use crate::utils::fzf::PreviewWindow;

#[derive(Clone)]
pub struct Mark {
    marks: Arc<Mutex<Option<HashMap<String, MarkItem>>>>,
}

impl Mark {
    pub fn new() -> Self {
        Mark {
            marks: Arc::new(Mutex::new(None)),
        }
    }
}

impl ModeDef for Mark {
    fn name(&self) -> &'static str {
        "mark"
    }
    fn load<'a>(
        &'a self,
        config: &Config,
        _state: &mut State,
        _query: String,
        _item: String,
    ) -> super::LoadStream<'a> {
        let nvim = config.nvim.clone();
        Box::pin(async_stream::stream! {
            let marks = get_nvim_marks(&nvim).await?;
            let items = marks.iter().map(|m| m.render()).collect();
            self.marks
                .lock()
                .await
                .replace(marks.into_iter().map(|b| (b.mark.clone(), b)).collect());
            yield Ok(LoadResp::new_with_default_header(items))
        })
    }
    fn preview<'a>(
        &'a self,
        _config: &Config,
        _win: &PreviewWindow,
        item: String,
    ) -> BoxFuture<'a, Result<PreviewResp>> {
        async move {
            let marks = self
                .marks
                .lock()
                .await
                .clone()
                .ok_or(anyhow!("marks not loaded"))?;
            let item = MarkItem::lookup(&marks, &item).ok_or(anyhow!("invalid item"))?;
            let file = shellexpand::tilde(&item.file).to_string();
            let message = bat::render_file_with_highlight(file, item.line as isize).await?;
            Ok(PreviewResp { message })
        }
        .boxed()
    }
    fn fzf_bindings(&self) -> (fzf::Bindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [{
                let self_ = self.clone();
                b.execute(move |_mode,config,_state,_query,item| {
                    let self_ = self_.clone();
                    async move {
                        let marks = self_.marks.lock().await.clone().ok_or(anyhow!("marks not loaded"))?;
                        let mark = MarkItem::lookup(&marks, &item)
                            .ok_or(anyhow!("invalid item"))?;
                        let file = shellexpand::tilde(&mark.file).to_string();
                        actions::open_in_nvim(config, file, Some(mark.line as usize), false).await
                    }.boxed()
                })
            }],
            "ctrl-t" => [{
                let self_ = self.clone();
                b.execute(move |_mode,config,_state,_query,item| {
                    let self_ = self_.clone();
                    async move {
                        let marks = self_.marks.lock().await.clone().ok_or(anyhow!("marks not loaded"))?;
                        let mark = MarkItem::lookup(&marks, &item)
                            .ok_or(anyhow!("invalid item"))?;
                        let file = shellexpand::tilde(&mark.file).to_string();
                        actions::open_in_nvim(config, file, Some(mark.line as usize), true).await
                    }.boxed()
                })
            }],
            "ctrl-y" => [
                execute!(b, |_mode,_config,_state,_query,item| {
                    let file = ITEM_PATTERN.replace(&item, "$file");
                    actions::yank(file).await
                })
            ],
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Util
////////////////////////////////////////////////////////////////////////////////////////////////////

async fn get_nvim_marks(nvim: &Neovim) -> Result<Vec<MarkItem>> {
    let marks: Vec<MarkItem> = from_value::<Vec<RawMarkItem>>(nvim.eval("getmarklist()").await?)?
        .into_iter()
        .map(|b| b.into())
        .collect();
    trace!("mark: get_nvim_marks: marks"; "marks" => Serde(marks.clone()));
    Ok(marks)
}


static ITEM_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?P<mark>\S+) (?P<file>\S*) (?P<line>\d+)").unwrap());

#[derive(Debug, Clone, Deserialize)]
struct RawMarkItem {
    mark: String,
    file: String,
    pos: [u64; 4], // [bufnr, line, col, off]
}

#[derive(Debug, Clone, Serialize)]
struct MarkItem {
    mark: String,
    line: u64,
    col: u64,
    file: String,
}

impl MarkItem {
    fn render(&self) -> String {
        format!("{:>2} {} {}", self.mark, self.file, self.line)
    }
    fn lookup(map: &HashMap<String, Self>, item: &str) -> Option<Self> {
        let c = ITEM_PATTERN.captures(item)?;
        let mark = c.name("mark")?.as_str().to_owned();
        map.get(&mark).cloned()
    }
}

impl From<RawMarkItem> for MarkItem {
    fn from(item: RawMarkItem) -> Self {
        MarkItem {
            mark: item.mark,
            line: item.pos[1],
            col: item.pos[2],
            file: item.file,
        }
    }
}
