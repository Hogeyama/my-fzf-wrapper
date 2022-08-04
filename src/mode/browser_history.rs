use std::process::ExitStatus;

use crate::{
    method::{Load, LoadResp, Method, PreviewResp, RunOpts, RunResp},
    types::{Mode, State},
};

use futures::{future::BoxFuture, FutureExt};
use once_cell::sync::Lazy;
use regex::Regex;
use rusqlite::{params, Connection};
use tokio::process::Command;

#[derive(Clone)]
pub struct BrowserHistory;

pub fn new() -> BrowserHistory {
    BrowserHistory
}

struct Item {
    url: String,
    title: String,
    date: String,
}

static ITEM_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?P<date>[^|]*)\|(?P<url>[^|]*)\|(?P<title>.*)"#).unwrap());

impl Mode for BrowserHistory {
    fn name(&self) -> &'static str {
        "browser_history"
    }
    fn load<'a>(
        &'a mut self,
        _state: &'a mut State,
        _opts: Vec<String>,
    ) -> BoxFuture<'a, <Load as Method>::Response> {
        async move {
            let pwd = std::env::current_dir().unwrap().into_os_string();
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
                    .unwrap()
                    .filter_map(|x| x.ok())
                    .collect::<Vec<_>>();
                Ok(items)
            };
            match tokio::task::spawn_blocking(run_query).await {
                Ok(Ok(items)) => LoadResp {
                    header: format!("[{}]", pwd.to_string_lossy()),
                    items: items
                        .into_iter()
                        .map(|x| format!("{}|{}|{}", x.date, x.url, x.title))
                        .collect(),
                },
                Ok(Err(e)) => {
                    error!("browser_history: error"; "error" => e.clone());
                    LoadResp {
                        header: format!("[{}]", pwd.to_string_lossy()),
                        items: vec![e],
                    }
                }
                Err(e) => {
                    error!("browser_history: tokio join error"; "error" => ?e);
                    LoadResp {
                        header: format!("[{}]", pwd.to_string_lossy()),
                        items: vec![],
                    }
                }
            }
        }
        .boxed()
    }
    fn preview(&mut self, _state: &mut State, item: String) -> BoxFuture<'static, PreviewResp> {
        async move {
            let url = ITEM_PATTERN.replace(&item, "$url").into_owned();
            let title = ITEM_PATTERN.replace(&item, "$title").into_owned();
            let date = ITEM_PATTERN.replace(&item, "$date").into_owned();
            let message = format!("URL:   {url}\nTITLE: {title}\nDATE:  {date}");
            PreviewResp { message }
        }
        .boxed()
    }
    fn run(
        &mut self,
        _state: &mut State,
        item: String,
        _opts: RunOpts,
    ) -> BoxFuture<'static, RunResp> {
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
            RunResp
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
    let home = std::env::var("HOME").unwrap();
    let path = format!("{}/.config/google-chrome/Profile 1/History", home);
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
                .unwrap();
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
            1000
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
            1000
    "#,
        "url LIKE 'https://%'"
    )
}
