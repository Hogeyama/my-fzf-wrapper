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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_path_item_returns_item_as_is() {
        let ext = FilePathItem;
        assert_eq!(ext.file("src/main.rs").unwrap(), "src/main.rs");
    }

    #[test]
    fn file_path_item_line_is_none() {
        let ext = FilePathItem;
        assert_eq!(ext.line("src/main.rs"), None);
    }

    // livegrep 形式: "file:line:col:content"
    static TEST_PATTERN: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?P<file>[^:]*):(?P<line>\d+):(?P<col>\d+):.*").unwrap());

    #[test]
    fn regex_item_extracts_file() {
        let ext = RegexItem {
            pattern: &TEST_PATTERN,
            file_group: "$file",
            line_group: Some("$line"),
        };
        assert_eq!(
            ext.file("src/main.rs:42:1:fn main()").unwrap(),
            "src/main.rs"
        );
    }

    #[test]
    fn regex_item_extracts_line() {
        let ext = RegexItem {
            pattern: &TEST_PATTERN,
            file_group: "$file",
            line_group: Some("$line"),
        };
        assert_eq!(ext.line("src/main.rs:42:1:fn main()"), Some(42));
    }

    #[test]
    fn regex_item_without_line_group() {
        let ext = RegexItem {
            pattern: &TEST_PATTERN,
            file_group: "$file",
            line_group: None,
        };
        assert_eq!(ext.line("src/main.rs:42:1:fn main()"), None);
    }

    #[test]
    fn regex_item_no_match_returns_original() {
        let ext = RegexItem {
            pattern: &TEST_PATTERN,
            file_group: "$file",
            line_group: Some("$line"),
        };
        // パターンにマッチしない場合、replace は元の文字列を返す
        assert_eq!(ext.file("no-match-here").unwrap(), "no-match-here");
    }
}
