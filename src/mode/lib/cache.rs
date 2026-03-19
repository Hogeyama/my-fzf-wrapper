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
