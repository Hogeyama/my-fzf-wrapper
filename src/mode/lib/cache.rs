use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use tokio::sync::Mutex;

/// モード固有のキャッシュ。load で書き込み、preview/execute で読み出す共通パターン。
#[derive(Clone)]
pub struct ModeCache<T> {
    inner: Arc<Mutex<Option<T>>>,
}

impl<T: Clone> ModeCache<T> {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
        }
    }

    /// load 時にデータをセット
    pub async fn set(&self, value: T) {
        *self.inner.lock().await = Some(value);
    }

    /// preview/execute 時にデータを取得。load 前なら Err。
    pub async fn get(&self) -> Result<T> {
        self.inner
            .lock()
            .await
            .clone()
            .ok_or_else(|| anyhow!("mode data not loaded yet"))
    }

    /// ロックを保持したままクロージャを実行（clone を避けたい場合）
    pub async fn with<R>(&self, f: impl FnOnce(&T) -> R) -> Result<R> {
        let guard = self.inner.lock().await;
        let data = guard
            .as_ref()
            .ok_or_else(|| anyhow!("mode data not loaded yet"))?;
        Ok(f(data))
    }

    #[allow(dead_code)]
    pub async fn clear(&self) {
        *self.inner.lock().await = None;
    }
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
}
