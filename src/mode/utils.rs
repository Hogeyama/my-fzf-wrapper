use crate::nvim::{self, Neovim};

// return whether change_directory happened
pub async fn change_directory(nvim: &Neovim, opts: CdOpts) -> Result<bool, String> {
    fn cd_to(path: &str) -> Result<(), String> {
        let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
        match std::fs::metadata(&path) {
            // std::fs::metadata は symlink を follow してくれる
            Ok(metadata) => {
                if metadata.is_dir() {
                    std::env::set_current_dir(path).map_err(|e| e.to_string())?;
                    Ok(())
                } else if metadata.is_file() {
                    let dir = path.parent().unwrap();
                    std::env::set_current_dir(dir).map_err(|e| e.to_string())?;
                    Ok(())
                } else {
                    let path = path.clone().into_os_string().into_string().unwrap();
                    Err(format!("path does not exists: {}", path))
                }
            }
            Err(_) => {
                let path = path.clone().into_os_string().into_string().unwrap();
                Err(format!("path does not exists: {}", path))
            }
        }
    }
    match opts {
        CdOpts { cd: Some(path), .. } => cd_to(&path).map(|_| true),
        CdOpts { cd_up, .. } if cd_up => {
            let mut dir = std::env::current_dir().unwrap();
            dir.pop();
            std::env::set_current_dir(dir)
                .map(|_| true)
                .map_err(|e| e.to_string())
        }
        CdOpts { cd_last_file, .. } if cd_last_file => {
            let last_file = nvim::last_opened_file(&nvim).await;
            match last_file {
                Ok(last_file) => cd_to(&last_file).map(|_| true),
                Err(e) => Err(e.to_string()),
            }
        }
        _ => Ok(false),
    }
}

pub struct CdOpts {
    // Change directory to the given dir.
    // If a file is specified, change to the directory containing the file.
    pub cd: Option<String>,

    // Change directory to the parent directory.
    pub cd_up: bool,

    // Change directory to the parent directory.
    pub cd_last_file: bool,
}
