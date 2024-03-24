use std::path::Path;

pub fn to_relpath(path: impl AsRef<Path>) -> String {
    let current_dir = std::env::current_dir().unwrap_or_default();
    let stripped_path = match path.as_ref().strip_prefix(&current_dir) {
        Ok(stripped_path) => stripped_path,
        Err(_) => path.as_ref(),
    };
    stripped_path
        .to_str()
        .expect("Invalid UTF-8 path")
        .to_string()
}
