use std::path::Path;

use anyhow::Result;
use rusqlite::params;
use rusqlite::Connection;
use rusqlite::Error;

pub fn run_query<T, F>(
    db: impl AsRef<Path>,
    temp_path: Option<impl AsRef<Path>>, // dbがロックされている可能性があるときに指定する
    query: &str,
    parse: F,
) -> Result<Vec<T>>
where
    F: FnMut(&rusqlite::Row<'_>) -> Result<T, Error>,
{
    let conn = match temp_path {
        Some(temp_path) => {
            std::fs::copy(db, temp_path.as_ref())?;
            Connection::open(temp_path)?
        }
        None => Connection::open(db)?,
    };
    let mut stmt = conn.prepare(query)?;
    let items = stmt
        .query_map(params![], parse)?
        .collect::<Vec<_>>()
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    Ok(items)
}
