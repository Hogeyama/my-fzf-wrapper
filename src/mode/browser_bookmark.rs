use anyhow::anyhow;
use anyhow::Result;
use futures::future::BoxFuture;
use futures::FutureExt;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use tokio::process::Command;

use crate::bindings;
use crate::config::Config;
use crate::method::LoadResp;
use crate::method::PreviewResp;
use crate::mode::config_builder;
use crate::mode::CallbackMap;
use crate::mode::ModeDef;
use crate::state::State;
use crate::utils::browser;
use crate::utils::fzf;
use crate::utils::fzf::PreviewWindow;
use crate::utils::sqlite;

#[derive(Clone)]
pub struct BrowserBookmark {
    browser: browser::Browser,
}

impl BrowserBookmark {
    pub fn new() -> Self {
        Self {
            browser: browser::get_browser(),
        }
    }
    async fn load_items(&self) -> Result<Vec<Item>> {
        match self.browser {
            browser::Browser::Firefox(_) => firefox_load_items(),
            browser::Browser::Chrome(_) => chrome_load_items(),
        }
    }
}

struct Item {
    title: String,
    url: String,
}

impl Item {
    fn render(&self) -> String {
        format!("{}|{}", self.title, self.url)
    }
    fn parse(item: String) -> Self {
        let title = ITEM_PATTERN.replace(&item, "$title").into_owned();
        let url = ITEM_PATTERN.replace(&item, "$url").into_owned();
        Item { title, url }
    }
}

static ITEM_PATTERN: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?P<title>.*)\|(?P<url>.*)").unwrap());

impl ModeDef for BrowserBookmark {
    fn name(&self) -> &'static str {
        "browser-bookmark"
    }
    fn load<'a>(
        &'a self,
        _config: &'a Config,
        _state: &'a mut State,
        _query: String,
        _item: String,
    ) -> super::LoadStream<'a> {
        Box::pin(async_stream::stream! {
            let items = self
                .load_items()
                .await?
                .into_iter()
                .map(|x| x.render())
                .collect();
            yield Ok(LoadResp::new_with_default_header(items))
        })
    }
    fn preview(
        &self,
        _config: &Config,
        _win: &PreviewWindow,
        item: String,
    ) -> BoxFuture<'static, Result<PreviewResp>> {
        async move {
            let Item { title, url } = Item::parse(item);
            let message = format!("URL:   {url}\nTITLE: {title}");
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
                b.execute(move |_mode,_config,_state,_query,item| {
                    let self_ = self_.clone();
                    async move {
                        let url = ITEM_PATTERN.replace(&item, "$url").into_owned();
                        Command::new(self_.browser.as_ref())
                            .arg(&url)
                            .spawn()
                            .expect("browser_history: open")
                            .wait()
                            .await
                            .expect("browser_history: open");
                        Ok(())
                    }.boxed()
                })
            }],
        }
    }
}

/////////////////////////////////////////////////////////////////////////////////
// Firefox
/////////////////////////////////////////////////////////////////////////////////

// 本当は enum Browser を trait Browser にして impl Firefox { } に書きたいんだが
// trait object は Clone できない問題があって妥協している。

fn firefox_load_items() -> Result<Vec<Item>> {
    let query = r#"
        SELECT
            moz_places.url,
            moz_bookmarks.title
        FROM
            moz_places
        INNER JOIN
            moz_bookmarks
          ON
            moz_places.id = moz_bookmarks.fk
        WHERE
            moz_places.url LIKE 'https://%'
          AND
            moz_places.url IS NOT NULL
          AND
            moz_places.url != ''
          AND
            moz_bookmarks.title IS NOT NULL
          AND
            moz_bookmarks.title != ''
        LIMIT
            10000
    "#;
    sqlite::run_query(
        firefox_db_path()?,
        Some("/tmp/fzfw_browser_bookmark.sqlite"),
        query,
        |row| {
            let url = row.get(0).unwrap();
            let title = row.get(1).unwrap();
            Ok(Item { url, title })
        },
    )
}

fn firefox_db_path() -> Result<String> {
    let home = std::env::var("HOME").unwrap();
    match std::fs::read_dir(format!("{home}/.mozilla/firefox")) {
        Ok(entries) => {
            let entry = entries
                .filter_map(|x| x.ok())
                .find(|x| x.file_name().to_string_lossy().ends_with(".default"))
                .ok_or(anyhow!("No firefox history found"))?;
            let dir = entry.path().to_string_lossy().to_string();
            Ok(dir + "/places.sqlite")
        }
        Err(_) => Err(anyhow!("Oh no! No firefox history found")),
    }
}

/////////////////////////////////////////////////////////////////////////////////
// Chrome
/////////////////////////////////////////////////////////////////////////////////

fn chrome_load_items() -> Result<Vec<Item>> {
    let json_path = chrome_json_path()?;
    let json = std::fs::read_to_string(json_path)?;
    let bookmark: Bookmark = serde_json::from_str(&json)?;
    let bookmark_bar_items = bookmark.roots.bookmark_bar.flatten();
    let other_items = bookmark.roots.other.flatten();
    let items = bookmark_bar_items
        .iter()
        .chain(other_items.iter())
        .map(|x| Item {
            title: x.title.clone(),
            url: x.url.clone(),
        })
        .collect();
    Ok(items)
}

fn chrome_json_path() -> Result<String> {
    let path = match std::env::var("FZFW_CHROME_BOOKMARKS_PATH") {
        Ok(path) => {
            info!("FZFW_CHROME_BOOKMARKS_PATH: {}", path);
            path
        }
        Err(_) => {
            let home = std::env::var("HOME").unwrap();
            let path = format!("{}/.config/google-chrome/Default/Bookmarks", home);
            path
        }
    };
    match std::fs::metadata(&path) {
        Ok(m) if m.is_file() => Ok(path),
        _ => Err(anyhow!("Oh no! No chrome history found")),
    }
}

#[derive(Debug, Deserialize)]
struct Bookmark {
    roots: BookmarkRoots,
}

#[derive(Debug, Deserialize)]
struct BookmarkRoots {
    bookmark_bar: BookmarkFolder,
    other: BookmarkFolder,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum BookmarkTree {
    Item(BookmarkItem),
    Folder(BookmarkFolder),
}

#[derive(Debug, Deserialize)]
struct BookmarkItem {
    #[serde(rename(deserialize = "name"))]
    title: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct BookmarkFolder {
    children: Vec<BookmarkTree>,
}

impl BookmarkFolder {
    fn flatten(&self) -> Vec<&BookmarkItem> {
        self.children.iter().flat_map(|x| x.flatten()).collect()
    }
}

impl BookmarkTree {
    fn flatten(&self) -> Vec<&BookmarkItem> {
        match self {
            BookmarkTree::Item(item) => vec![item],
            BookmarkTree::Folder(folder) => folder.flatten(),
        }
    }
}
