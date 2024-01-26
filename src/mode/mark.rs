use std::{collections::HashMap, error::Error, sync::Arc};

use crate::{
    config::Config,
    external_command::{bat, fzf, xsel},
    logger::Serde,
    method::{LoadResp, PreviewResp},
    mode::{config_builder, ModeDef},
    nvim::{self, Neovim, NeovimExt},
    state::State,
};

use futures::{future::BoxFuture, FutureExt};
use once_cell::sync::Lazy;
use regex::Regex;
use rmpv::ext::from_value;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use super::CallbackMap;

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
        &'a mut self,
        _config: &Config,
        state: &mut State,
        _query: String,
        _item: String,
    ) -> BoxFuture<'a, Result<LoadResp, String>> {
        let nvim = state.nvim.clone();
        async move {
            let marks = get_nvim_marks(&nvim).await.map_err(|e| e.to_string())?;
            let items = marks.iter().map(|m| m.render()).collect();
            self.marks
                .lock()
                .await
                .replace(marks.into_iter().map(|b| (b.mark.clone(), b)).collect());
            Ok(LoadResp::new_with_default_header(items))
        }
        .boxed()
    }
    fn preview<'a>(
        &'a self,
        _config: &Config,
        _state: &mut State,
        item: String,
    ) -> BoxFuture<'a, Result<PreviewResp, String>> {
        async move {
            let marks = self.marks.lock().await.clone().ok_or("marks not loaded")?;
            let item = MarkItem::lookup(&marks, &item).ok_or("invalid item")?;
            let file = shellexpand::tilde(&item.file).to_string();
            let message = bat::render_file_with_highlight(file, item.line as isize).await?;
            Ok(PreviewResp { message })
        }
        .boxed()
    }
    fn fzf_bindings<'a>(&'a self) -> (fzf::Bindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [{
                let self_ = self.clone();
                b.execute(move |_mode,_config,state,_query,item| {
                    // https://github.com/rust-lang/rust/issues/68119#issuecomment-1293231676
                    // TODO よく分かってない
                    let self_ = self_.clone();
                    async move {
                        let marks = self_.marks.lock().await.clone().ok_or("marks not loaded")?;
                        let mark = MarkItem::lookup(&marks, &item)
                            .ok_or("invalid item")?;
                        let opts = ExecOpts::Open { tabedit: false };
                        exec(mark, state, opts).await
                    }.boxed()
                })
            }],
            "ctrl-t" => [{
                let self_ = self.clone();
                b.execute(move |_mode,_config,state,_query,item| {
                    let self_ = self_.clone();
                    async move {
                        let marks = self_.marks.lock().await.clone().ok_or("marks not loaded")?;
                        let mark = MarkItem::lookup(&marks, &item)
                            .ok_or("invalid item")?;
                        let opts = ExecOpts::Open { tabedit: true };
                        exec(mark, state, opts).await
                    }.boxed()
                })
            }],
            "ctrl-y" => [
                execute!(b, |_mode,_config,_state,_query,item| {
                    let file = ITEM_PATTERN.replace(&item, "$file");
                    xsel::yank(file).await?;
                    Ok(())
                })
            ],
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Util
////////////////////////////////////////////////////////////////////////////////////////////////////

async fn get_nvim_marks(nvim: &Neovim) -> Result<Vec<MarkItem>, Box<dyn Error>> {
    let marks: Vec<MarkItem> = from_value::<Vec<RawMarkItem>>(nvim.eval("getmarklist()").await?)?
        .into_iter()
        .map(|b| b.into())
        .collect();
    trace!("mark: get_nvim_marks: marks"; "marks" => Serde(marks.clone()));
    Ok(marks)
}

enum ExecOpts {
    Open { tabedit: bool },
}

async fn exec(mark: MarkItem, state: &mut State, opts: ExecOpts) -> Result<(), String> {
    match opts {
        ExecOpts::Open { tabedit } => {
            let nvim = state.nvim.clone();
            let nvim_opts = nvim::OpenOpts {
                line: Some(mark.line as usize),
                tabedit,
            };
            let file = shellexpand::tilde(&mark.file).to_string();
            let r = nvim.open(file.into(), nvim_opts).await;
            if let Err(e) = r {
                error!("buffer: run: nvim_open failed"; "error" => e.to_string());
            }
        }
    }
    Ok(())
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
