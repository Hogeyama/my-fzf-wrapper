use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::sync::Mutex;

type SerializeFn<T> = Box<dyn Fn(&T) -> Result<String> + Send + Sync>;
type DeserializeFn<T> = Box<dyn Fn(&str) -> Result<T> + Send + Sync>;

/// Type-erased persistence backend.
/// Captured at `persisted()` / `enable_persistence()` time so that
/// `set` / `get` / `with` / `with_mut` do NOT require Serialize/DeserializeOwned bounds.
struct PersistenceBackend<T> {
    file_path: PathBuf,
    serialize: SerializeFn<T>,
    deserialize: DeserializeFn<T>,
}

/// namespace/key のパスコンポーネントをサニタイズする。
/// 不正な文字（`/`, `\`, null バイト）や `..` を含む場合はエラーを返す。
fn sanitize_path_component(component: &str) -> Result<()> {
    if component.is_empty() {
        return Err(anyhow!("path component must not be empty"));
    }
    if component == ".." || component == "." {
        return Err(anyhow!(
            "path component must not be '.' or '..': {:?}",
            component
        ));
    }
    if component.contains('/') || component.contains('\\') || component.contains('\0') {
        return Err(anyhow!(
            "path component contains invalid character: {:?}",
            component
        ));
    }
    Ok(())
}

/// $XDG_STATE_HOME/fzfw を返す。未設定時は ~/.local/state/fzfw。
fn state_dir() -> PathBuf {
    match std::env::var("XDG_STATE_HOME") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir).join("fzfw"),
        _ => {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".local/state/fzfw")
        }
    }
}

fn build_backend_with_base<T: Serialize + DeserializeOwned + 'static>(
    base_dir: PathBuf,
    namespace: &str,
    key: &str,
) -> Result<PersistenceBackend<T>> {
    sanitize_path_component(namespace)?;
    sanitize_path_component(key)?;
    let file_path = base_dir.join(namespace).join(format!("{}.json", key));
    Ok(PersistenceBackend {
        file_path,
        serialize: Box::new(|value| Ok(serde_json::to_string_pretty(value)?)),
        deserialize: Box::new(|s| Ok(serde_json::from_str(s)?)),
    })
}

fn build_backend<T: Serialize + DeserializeOwned + 'static>(
    namespace: &str,
    key: &str,
) -> Result<PersistenceBackend<T>> {
    build_backend_with_base(state_dir(), namespace, key)
}

/// モード固有のキャッシュ。load で書き込み、preview/execute で読み出す共通パターン。
#[derive(Clone)]
pub struct ModeCache<T> {
    inner: Arc<Mutex<Option<T>>>,
    persistence: Arc<Mutex<Option<PersistenceBackend<T>>>>,
}

impl<T: Clone> ModeCache<T> {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
            persistence: Arc::new(Mutex::new(None)),
        }
    }

    /// 永続化付きで初期化する。namespace と key はディレクトリ/ファイル名に使われる。
    /// 保存先: $XDG_STATE_HOME/fzfw/{namespace}/{key}.json
    #[allow(dead_code)]
    pub fn persisted(namespace: &str, key: &str) -> Result<Self>
    where
        T: Serialize + DeserializeOwned + 'static,
    {
        let backend = build_backend(namespace, key)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(None)),
            persistence: Arc::new(Mutex::new(Some(backend))),
        })
    }

    /// 後から永続化を有効化する。
    #[allow(dead_code)]
    pub async fn enable_persistence(&self, namespace: &str, key: &str) -> Result<()>
    where
        T: Serialize + DeserializeOwned + 'static,
    {
        let backend = build_backend(namespace, key)?;
        *self.persistence.lock().await = Some(backend);
        Ok(())
    }

    /// load 時にデータをセット
    pub async fn set(&self, value: T) {
        let mut inner = self.inner.lock().await;
        *inner = Some(value);
        // 永続化が有効なら自動保存（同一ロックスコープ内で行う）
        let persistence = self.persistence.lock().await;
        if let (Some(backend), Some(value)) = (persistence.as_ref(), inner.as_ref()) {
            match (backend.serialize)(value) {
                Ok(json) => {
                    if let Err(e) = write_file(&backend.file_path, &json) {
                        crate::warn!("failed to persist cache: {}", e);
                    }
                }
                Err(e) => {
                    crate::warn!("failed to serialize cache: {}", e);
                }
            }
        }
    }

    /// preview/execute 時にデータを取得。load 前なら Err。
    /// 永続化が有効でインメモリにデータがない場合、ファイルから復元を試みる。
    pub async fn get(&self) -> Result<T> {
        self.maybe_restore().await;
        self.inner
            .lock()
            .await
            .clone()
            .ok_or_else(|| anyhow!("mode data not loaded yet"))
    }

    /// ロックを保持したままクロージャを実行（clone を避けたい場合）
    pub async fn with<R>(&self, f: impl FnOnce(&T) -> R) -> Result<R> {
        self.maybe_restore().await;
        let guard = self.inner.lock().await;
        let data = guard
            .as_ref()
            .ok_or_else(|| anyhow!("mode data not loaded yet"))?;
        Ok(f(data))
    }

    /// ロックを保持したまま可変クロージャを実行
    pub async fn with_mut<R>(&self, f: impl FnOnce(&mut T) -> R) -> Result<R> {
        self.maybe_restore().await;
        let mut guard = self.inner.lock().await;
        let data = guard
            .as_mut()
            .ok_or_else(|| anyhow!("mode data not loaded yet"))?;
        let result = f(data);
        // with_mut 後に自動保存
        let persistence = self.persistence.lock().await;
        if let (Some(backend), Some(value)) = (persistence.as_ref(), guard.as_ref()) {
            match (backend.serialize)(value) {
                Ok(json) => {
                    if let Err(e) = write_file(&backend.file_path, &json) {
                        crate::warn!("failed to persist cache: {}", e);
                    }
                }
                Err(e) => {
                    crate::warn!("failed to serialize cache: {}", e);
                }
            }
        }
        Ok(result)
    }

    #[allow(dead_code)]
    pub async fn clear(&self) {
        *self.inner.lock().await = None;
    }

    /// 永続化ファイルを削除する。
    #[allow(dead_code)]
    pub async fn delete_file(&self) {
        let persistence = self.persistence.lock().await;
        if let Some(backend) = persistence.as_ref() {
            if backend.file_path.exists() {
                if let Err(e) = std::fs::remove_file(&backend.file_path) {
                    crate::warn!("failed to delete cache file: {}", e);
                }
            }
        }
    }

    /// インメモリにデータがなく永続化が有効な場合、ファイルから復元を試みる。
    async fn maybe_restore(&self) {
        let mut inner = self.inner.lock().await;
        if inner.is_some() {
            return;
        }
        let persistence = self.persistence.lock().await;
        if let Some(backend) = persistence.as_ref() {
            if !backend.file_path.exists() {
                return;
            }
            match std::fs::read_to_string(&backend.file_path) {
                Ok(contents) => match (backend.deserialize)(&contents) {
                    Ok(value) => {
                        *inner = Some(value);
                    }
                    Err(e) => {
                        crate::warn!("failed to deserialize cache from file: {}", e);
                    }
                },
                Err(e) => {
                    crate::warn!("failed to read cache file: {}", e);
                }
            }
        }
    }
}

fn write_file(path: &std::path::Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn get_before_set_returns_error() {
        let cache: ModeCache<String> = ModeCache::new();
        assert!(cache.get().await.is_err());
    }

    #[tokio::test]
    async fn set_then_get() {
        let cache: ModeCache<String> = ModeCache::new();
        cache.set("hello".to_string()).await;
        assert_eq!(cache.get().await.unwrap(), "hello");
    }

    #[tokio::test]
    async fn set_overwrites_previous() {
        let cache: ModeCache<i32> = ModeCache::new();
        cache.set(1).await;
        cache.set(2).await;
        assert_eq!(cache.get().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn with_applies_closure() {
        let cache: ModeCache<Vec<i32>> = ModeCache::new();
        cache.set(vec![1, 2, 3]).await;
        let len = cache.with(|v| v.len()).await.unwrap();
        assert_eq!(len, 3);
    }

    #[tokio::test]
    async fn with_before_set_returns_error() {
        let cache: ModeCache<String> = ModeCache::new();
        assert!(cache.with(|_| ()).await.is_err());
    }

    #[tokio::test]
    async fn clear_resets_to_none() {
        let cache: ModeCache<String> = ModeCache::new();
        cache.set("data".to_string()).await;
        cache.clear().await;
        assert!(cache.get().await.is_err());
    }

    #[tokio::test]
    async fn clone_shares_state() {
        let cache: ModeCache<String> = ModeCache::new();
        let cache2 = cache.clone();
        cache.set("shared".to_string()).await;
        assert_eq!(cache2.get().await.unwrap(), "shared");
    }

    // --- Persistence tests ---

    /// テスト用: 指定したベースディレクトリで永続化付き ModeCache を作成する。
    fn persisted_in<T: Clone + Serialize + DeserializeOwned + 'static>(
        base_dir: PathBuf,
        namespace: &str,
        key: &str,
    ) -> ModeCache<T> {
        let backend = build_backend_with_base(base_dir, namespace, key).unwrap();
        ModeCache {
            inner: Arc::new(Mutex::new(None)),
            persistence: Arc::new(Mutex::new(Some(backend))),
        }
    }

    #[tokio::test]
    async fn persisted_set_creates_file() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("fzfw");
        let cache: ModeCache<Vec<String>> = persisted_in(base, "test-ns", "test-key");
        cache.set(vec!["a".to_string(), "b".to_string()]).await;
        let file_path = tmp.path().join("fzfw/test-ns/test-key.json");
        assert!(file_path.exists(), "persisted file should exist after set");
    }

    #[tokio::test]
    async fn persisted_restores_on_get() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("fzfw");

        // First instance: set value
        let cache1: ModeCache<Vec<i32>> = persisted_in(base.clone(), "restore-ns", "restore-key");
        cache1.set(vec![10, 20, 30]).await;

        // Second instance: should restore from file
        let cache2: ModeCache<Vec<i32>> = persisted_in(base, "restore-ns", "restore-key");
        let val = cache2.get().await.unwrap();
        assert_eq!(val, vec![10, 20, 30]);
    }

    #[tokio::test]
    async fn persisted_with_mut_auto_saves() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("fzfw");

        let cache: ModeCache<Vec<String>> = persisted_in(base, "mut-ns", "mut-key");
        cache.set(vec!["original".to_string()]).await;

        cache
            .with_mut(|v| v.push("added".to_string()))
            .await
            .unwrap();

        // Read file and verify
        let file_path = tmp.path().join("fzfw/mut-ns/mut-key.json");
        let contents = std::fs::read_to_string(&file_path).unwrap();
        let restored: Vec<String> = serde_json::from_str(&contents).unwrap();
        assert_eq!(restored, vec!["original", "added"]);
    }

    #[tokio::test]
    async fn persisted_delete_file_removes() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("fzfw");

        let cache: ModeCache<String> = persisted_in(base, "del-ns", "del-key");
        cache.set("to-delete".to_string()).await;

        let file_path = tmp.path().join("fzfw/del-ns/del-key.json");
        assert!(file_path.exists());

        cache.delete_file().await;
        assert!(
            !file_path.exists(),
            "file should be removed after delete_file"
        );
    }

    #[tokio::test]
    async fn persisted_corrupt_file_returns_none() {
        let tmp = tempfile::tempdir().unwrap();

        // Write corrupt JSON directly
        let dir = tmp.path().join("fzfw/corrupt-ns");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("corrupt-key.json"), "not valid json{{{").unwrap();

        let base = tmp.path().join("fzfw");
        let cache: ModeCache<Vec<i32>> = persisted_in(base, "corrupt-ns", "corrupt-key");
        // get should return Err (not loaded) because corrupt file is treated as None
        assert!(cache.get().await.is_err());
    }

    #[test]
    fn sanitize_rejects_traversal() {
        assert!(sanitize_path_component(".").is_err());
        assert!(sanitize_path_component("..").is_err());
        assert!(sanitize_path_component("foo/bar").is_err());
        assert!(sanitize_path_component("foo\\bar").is_err());
        assert!(sanitize_path_component("foo\0bar").is_err());
        assert!(sanitize_path_component("").is_err());
        // Valid ones
        assert!(sanitize_path_component("valid-name").is_ok());
        assert!(sanitize_path_component("valid_name").is_ok());
        assert!(sanitize_path_component("valid.name").is_ok());
        assert!(sanitize_path_component(".hidden").is_ok());
    }
}
