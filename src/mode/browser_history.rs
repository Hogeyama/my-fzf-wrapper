use std::process::ExitStatus;

use crate::{
    method::{LoadResp, PreviewResp, RunOpts, RunResp},
    types::{Mode, State},
};

use futures::{future::BoxFuture, FutureExt};
use once_cell::sync::Lazy;
use regex::Regex;
use rusqlite::{params, Connection};
use tokio::process::Command;

#[derive(Clone)]
pub struct BrowserHistory;

struct Item {
    url: String,
    title: String,
    date: String,
}

static ITEM_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?P<date>[^|]*)\|(?P<url>[^|]*)\|(?P<title>.*)").unwrap());

impl Mode for BrowserHistory {
    fn new() -> Self {
        BrowserHistory
    }
    fn name(&self) -> &'static str {
        "browser-history"
    }
    fn load<'a>(
        &'a self,
        _state: &'a mut State,
        _opts: Vec<String>,
    ) -> BoxFuture<'a, Result<LoadResp, String>> {
        async move {
            let run_query = || -> Result<Vec<Item>, String> {
                let (db, query) = if get_browser().eq("firefox") {
                    info!("oh yes");
                    (get_firefox_db_path()?, firefox_query())
                } else {
                    (get_chrome_db_path()?, chrome_query())
                };
                std::fs::copy(db, temp_sqlite_path()).map_err(|e| e.to_string())?;
                let conn = Connection::open(temp_sqlite_path()).map_err(|e| e.to_string())?;
                let mut stmt = conn.prepare(&query).map_err(|e| e.to_string())?;
                let items = stmt
                    .query_map(params![], |row| {
                        let url = row.get(0).unwrap();
                        let title = row.get(1).unwrap();
                        let date = row.get(2).unwrap();
                        Ok(Item { url, title, date })
                    })
                    .map_err(|e| e.to_string())?
                    .filter_map(|x| x.ok())
                    .collect::<Vec<_>>();
                Ok(items)
            };
            let items = tokio::task::spawn_blocking(run_query)
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
    fn run(
        &self,
        _state: &mut State,
        item: String,
        _opts: RunOpts,
    ) -> BoxFuture<'static, Result<RunResp, String>> {
        async move {
            let url = ITEM_PATTERN.replace(&item, "$url").into_owned();
            let browser = get_browser();
            let _: ExitStatus = Command::new(browser)
                .arg(&url)
                .spawn()
                .expect("browser_history: open")
                .wait()
                .await
                .expect("browser_history: open");
            Ok(RunResp)
        }
        .boxed()
    }
}

fn get_browser() -> String {
    vec![
        std::env::var("FZFW_BROWSER"),
        std::env::var("BROWSER"),
        Ok("firefox".to_string()),
    ]
    .into_iter()
    .find(|x| x.is_ok())
    .unwrap()
    .unwrap()
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
