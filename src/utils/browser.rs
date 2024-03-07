#[derive(Clone)]
pub enum Browser {
    Firefox(String),
    Chrome(String),
}

impl AsRef<str> for Browser {
    fn as_ref(&self) -> &str {
        match self {
            Browser::Firefox(s) => s,
            Browser::Chrome(s) => s,
        }
    }
}

pub fn get_browser() -> Browser {
    let browser = vec![std::env::var("FZFW_BROWSER"), std::env::var("BROWSER")]
        .into_iter()
        .find(|x| x.is_ok());
    if let Some(Ok(browser)) = browser {
        if browser.contains("chrome") {
            Browser::Chrome(browser)
        } else {
            Browser::Firefox(browser)
        }
    } else {
        Browser::Firefox("firefox".to_string())
    }
}
