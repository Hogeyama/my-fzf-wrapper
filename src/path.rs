pub fn to_relpath(path: impl AsRef<str>) -> String {
    let pwd = std::env::current_dir().unwrap();
    let pwd = pwd.to_str().unwrap();
    path.as_ref().replace(&format!("{pwd}/"), "")
}
