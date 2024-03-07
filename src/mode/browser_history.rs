use crate::{
    bindings,
    config::Config,
    method::{LoadResp, PreviewResp},
    mode::{config_builder, ModeDef},
    state::State,
    utils::{browser, fzf, sqlite},
};

use futures::{future::BoxFuture, FutureExt};
use once_cell::sync::Lazy;
use regex::Regex;
use tokio::process::Command;

use super::CallbackMap;

#[derive(Clone)]
pub struct BrowserHistory {
    browser: browser::Browser,
}

impl BrowserHistory {
    pub fn new() -> Self {
        Self {
            browser: browser::get_browser(),
        }
    }
}

struct Item {
    url: String,
    title: String,
    date: String,
}

static ITEM_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?P<date>[^|]*)\|(?P<url>[^|]*)\|(?P<title>.*)").unwrap());

impl ModeDef for BrowserHistory {
    fn name(&self) -> &'static str {
        "browser-history"
    }
    fn load<'a>(
        &'a mut self,
        _config: &'a Config,
        _state: &'a mut State,
        _query: String,
        _item: String,
    ) -> BoxFuture<'a, Result<LoadResp, String>> {
        async move {
            let (db, query) = match self.browser {
                browser::Browser::Firefox(_) => (get_firefox_db_path()?, firefox_query()),
                browser::Browser::Chrome(_) => (get_chrome_db_path()?, chrome_query()),
            };
            let items = tokio::task::spawn_blocking(move || {
                sqlite::run_query(db, Some(temp_sqlite_path()), &query, |row| {
                    let url = row.get(0).unwrap();
                    let title = row.get(1).unwrap();
                    let date = row.get(2).unwrap();
                    Ok(Item { url, title, date })
                })
            })
            .await
            .map_err(|e| e.to_string())??
            .into_iter()
            .map(|x| format!("{}|{}|{}", x.date, x.url, x.title))
            .collect();
            Ok(LoadResp::new_with_default_header(items))
        }
        .boxed()
    }
    fn preview(
        &self,
        _config: &Config,
        _state: &mut State,
        item: String,
    ) -> BoxFuture<'static, Result<PreviewResp, String>> {
        async move {
            let url = ITEM_PATTERN.replace(&item, "$url").into_owned();
            let title = ITEM_PATTERN.replace(&item, "$title").into_owned();
            let date = ITEM_PATTERN.replace(&item, "$date").into_owned();
            let message = format!("URL:   {url}\nTITLE: {title}\nDATE:  {date}");
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

fn temp_sqlite_path() -> &'static str {
    "/tmp/fzfw_browser_history.sqlite"
}

fn get_chrome_db_path() -> Result<String, String> {
    // FIXME ad-hoc
    let path = match std::env::var("FZFW_CHROME_HISTORY_PATH") {
        Ok(path) => path,
        Err(_) => {
            let home = std::env::var("HOME").unwrap();
            let path = format!("{}/.config/google-chrome/Profile 1/History", home);
            path
        }
    };
    match std::fs::metadata(&path) {
        Ok(m) if m.is_file() => Ok(path),
        _ => Err("Oh no! No chrome history found".to_string()),
    }
}

fn get_firefox_db_path() -> Result<String, String> {
    let home = std::env::var("HOME").unwrap();
    match std::fs::read_dir(format!("{home}/.mozilla/firefox")) {
        Ok(entries) => {
            let entry = entries
                .filter_map(|x| x.ok())
                .find(|x| x.file_name().to_string_lossy().ends_with(".default"))
                .ok_or("No firefox history found".to_string())?;
            let dir = entry.path().to_string_lossy().to_string();
            Ok(dir + "/places.sqlite")
        }
        Err(_) => Err("Oh no! No firefox history found".to_string()),
    }
}

fn chrome_query() -> String {
    format!(
        r#"
        SELECT url
             , title
             , DATETIME(last_visit_time / 1000000 + (strftime('%s', '1601-01-01') ), 'unixepoch', '+9 hours') AS date 
        FROM
            urls
        WHERE
            {}
        GROUP BY
            title
        ORDER BY
            date DESC
        LIMIT
            10000
    "#,
        "url LIKE 'https://%'"
    )
}

fn firefox_query() -> String {
    format!(
        r#"
        SELECT
            url,
            title,
            DATETIME(last_visit_date / 1000000, 'unixepoch', '+9 hours') AS date
        FROM
            moz_places
        WHERE
            {}
        GROUP BY
            title
        ORDER BY
            date DESC
        LIMIT
            10000
    "#,
        "url LIKE 'https://%'"
    )
}
