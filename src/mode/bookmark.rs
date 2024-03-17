use std::error::Error;

use crate::{
    config::Config,
    method::{LoadResp, PreviewResp},
    mode::{config_builder, ModeDef},
    nvim::{self, Neovim, NeovimExt},
    path::to_relpath,
    state::State,
    utils::{bat, fzf, xsel},
};

use futures::{future::BoxFuture, FutureExt};
use rmpv::ext::from_value;
use serde::Serialize;

use super::CallbackMap;

#[derive(Clone)]
pub struct Bookmark;

impl Bookmark {
    pub fn new() -> Self {
        Bookmark
    }
}

impl ModeDef for Bookmark {
    fn name(&self) -> &'static str {
        "bookmark"
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
            let bookmarks = get_bookmarks(&nvim).await.map_err(|e| e.to_string())?;
            let items = bookmarks.iter().map(|m| m.render()).collect();
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
            let bookmark = BookmarkItem::parse(&item)?;
            let message =
                bat::render_file_with_highlight(bookmark.file, bookmark.line as isize).await?;
            Ok(PreviewResp { message })
        }
        .boxed()
    }
    fn fzf_bindings(&self) -> (fzf::Bindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [
                execute_silent!(b, |_mode,_config,state,_query,item| {
                    let bookmark = BookmarkItem::parse(&item)?;
                    let opts = ExecOpts::Open { tabedit: false };
                    open(bookmark, state, opts).await
                })
            ],
            "ctrl-t" => [
                execute_silent!(b, |_mode,_config,state,_query,item| {
                    let bookmark = BookmarkItem::parse(&item)?;
                    let opts = ExecOpts::Open { tabedit: true };
                    open(bookmark, state, opts).await
                })
            ],
            "ctrl-y" => [
                execute_silent!(b, |_mode,_config,_state,_query,item| {
                    let bookmark = BookmarkItem::parse(&item)?;
                    xsel::yank(bookmark.file).await?;
                    Ok(())
                })
            ],
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Util
////////////////////////////////////////////////////////////////////////////////////////////////////

async fn get_bookmarks(nvim: &Neovim) -> Result<Vec<BookmarkItem>, Box<dyn Error>> {
    // Example:
    // [ "/home/hogeyama/code/my-fzf-wrapper/src/mode/bookmark.rs:23:pub struct Bookmark {"
    // , "/home/hogeyama/code/my-fzf-wrapper/src/mode/bookmark.rs:27:impl Bookmark {"
    // ]
    let bookmarks: Vec<String> = from_value::<Vec<String>>(nvim.eval("bm#location_list()").await?)?;
    let bookmarks = bookmarks
        .iter()
        .map(|b| {
            let mut parts = b.split(':');
            let file = to_relpath(parts.next().unwrap());
            let line = parts.next().unwrap().parse().unwrap();
            BookmarkItem { file, line }
        })
        .collect::<Vec<_>>();
    Ok(bookmarks)
}

enum ExecOpts {
    Open { tabedit: bool },
}

async fn open(bookmark: BookmarkItem, state: &mut State, opts: ExecOpts) -> Result<(), String> {
    match opts {
        ExecOpts::Open { tabedit } => {
            let nvim = state.nvim.clone();
            let nvim_opts = nvim::OpenOpts {
                line: Some(bookmark.line as usize),
                tabedit,
            };
            let r = nvim.open(bookmark.file.clone().into(), nvim_opts).await;
            if let Err(e) = r {
                error!("buffer: run: nvim_open failed"; "error" => e.to_string());
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
struct BookmarkItem {
    file: String,
    line: u64,
}

impl BookmarkItem {
    fn render(&self) -> String {
        format!("{}:{}", self.file, self.line)
    }
    fn parse(s: &str) -> Result<Self, String> {
        let (file, line) = s.rsplit_once(':').ok_or("invalid item")?;
        let file = file.to_string();
        let line = line.parse().ok().ok_or("invalid item")?;
        Ok(BookmarkItem { file, line })
    }
}
