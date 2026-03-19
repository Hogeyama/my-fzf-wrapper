use anyhow::Result;
use once_cell::sync::Lazy;
use regex::Regex;

/// アイテム文字列からファイルパスと行番号を抽出するトレイト。
/// ConfigBuilder のファイルオープン系ヘルパーで使用する。
pub trait ItemExtractor: Clone + Send + Sync + 'static {
    fn file(&self, item: &str) -> Result<String>;
    fn line(&self, item: &str) -> Option<usize> {
        let _ = item;
        None
    }
}

/// item がそのままファイルパスであるモード用 (fd, mru, visits)
#[derive(Clone)]
pub struct FilePathItem;

impl ItemExtractor for FilePathItem {
    fn file(&self, item: &str) -> Result<String> {
        Ok(item.to_string())
    }
}

/// 正規表現の名前付きキャプチャでファイルパスと行番号を抽出するモード用 (livegrep)
#[derive(Clone, Copy)]
pub struct RegexItem {
    pub pattern: &'static Lazy<Regex>,
    pub file_group: &'static str,
    pub line_group: Option<&'static str>,
}

impl ItemExtractor for RegexItem {
    fn file(&self, item: &str) -> Result<String> {
        Ok(self.pattern.replace(item, self.file_group).into_owned())
    }
    fn line(&self, item: &str) -> Option<usize> {
        self.line_group
            .map(|g| self.pattern.replace(item, g).into_owned())
            .and_then(|s| s.parse().ok())
    }
}
