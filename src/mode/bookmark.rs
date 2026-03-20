use anyhow::anyhow;
use anyhow::Result;
use futures::future::BoxFuture;
use futures::FutureExt;
use rmpv::ext::from_value;
use serde::Serialize;

use super::lib::item::ItemExtractor;
use crate::env::Env;
use crate::method::LoadResp;
use crate::method::PreviewResp;
use crate::mode::config_builder;
use crate::mode::CallbackMap;
use crate::mode::ModeDef;
use crate::nvim::Neovim;
use crate::state::State;
use crate::utils::bat;
use crate::utils::fzf::PreviewWindow;
use crate::utils::path::to_relpath;

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
        &'a self,
        env: &Env,
        _state: &mut State,
        _query: String,
        _item: String,
    ) -> super::LoadStream<'a> {
        let nvim = env.nvim.clone();
        Box::pin(async_stream::stream! {
            let bookmarks = get_bookmarks(&nvim).await?;
            let items = bookmarks.iter().map(|m| m.render()).collect();
            yield Ok(LoadResp::new_with_default_header(items))
        })
    }
    fn preview<'a>(
        &'a self,
        _env: &Env,
        _win: &PreviewWindow,
        item: String,
    ) -> BoxFuture<'a, Result<PreviewResp>> {
        async move {
            let bookmark = BookmarkItem::parse(&item)?;
            let message =
                bat::render_file_with_highlight(bookmark.file, bookmark.line as isize).await?;
            Ok(PreviewResp { message })
        }
        .boxed()
    }
    fn fzf_bindings(&self) -> (super::ModeBindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [ b.open_nvim_silent(BookmarkExtractor, false) ],
            "ctrl-t" => [ b.open_nvim_silent(BookmarkExtractor, true) ],
            "ctrl-y" => [ b.yank_file(BookmarkExtractor) ],
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Util
////////////////////////////////////////////////////////////////////////////////////////////////////

async fn get_bookmarks(nvim: &Neovim) -> Result<Vec<BookmarkItem>> {
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

#[derive(Debug, Clone, Serialize)]
struct BookmarkItem {
    file: String,
    line: u64,
}

impl BookmarkItem {
    fn render(&self) -> String {
        format!("{}:{}", self.file, self.line)
    }
    fn parse(s: &str) -> Result<Self> {
        let (file, line) = s.rsplit_once(':').ok_or(anyhow!("invalid item"))?;
        let file = file.to_string();
        let line = line.parse().ok().ok_or(anyhow!("invalid item"))?;
        Ok(BookmarkItem { file, line })
    }
}

/// bookmark のアイテム文字列 ("file:line") からファイルと行番号を抽出する
#[derive(Clone)]
struct BookmarkExtractor;

impl ItemExtractor for BookmarkExtractor {
    fn file(&self, item: &str) -> Result<String> {
        Ok(BookmarkItem::parse(item)?.file)
    }
    fn line(&self, item: &str) -> Option<usize> {
        BookmarkItem::parse(item).ok().map(|b| b.line as usize)
    }
}
