use std::path::Path;

use rusqlite::{params, Connection, Error};

pub fn run_query<T, F>(
    db: impl AsRef<Path>,
    temp_path: Option<impl AsRef<Path>>, // dbがロックされている可能性があるときに指定する
    query: &str,
    parse: F,
) -> Result<Vec<T>, String>
where
    F: FnMut(&rusqlite::Row<'_>) -> Result<T, Error>,
{
    let conn = match temp_path {
        Some(temp_path) => {
            std::fs::copy(db, temp_path.as_ref()).map_err(|e| e.to_string())?;
            Connection::open(temp_path).map_err(|e| e.to_string())?
        }
        None => Connection::open(db).map_err(|e| e.to_string())?,
    };
    let mut stmt = conn.prepare(query).map_err(|e| e.to_string())?;
    let items = stmt
        .query_map(params![], parse)
        .map_err(|e| e.to_string())?
        .collect::<Vec<_>>()
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(items)
}
