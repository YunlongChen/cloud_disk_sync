mod aliyun;

use crate::config::RateLimitConfig;
use crate::core::traits::RateLimiter;
use crate::error::SyncError;
use async_trait::async_trait;
use std::path::Path;
use std::time::Duration;

#[async_trait]
pub trait StorageProvider: Send + Sync {
    async fn list(&self, path: &str) -> Result<Vec<FileInfo>, SyncError>;
    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<UploadResult, SyncError>;
    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<DownloadResult, SyncError>;
    async fn delete(&self, path: &str) -> Result<(), SyncError>;
    async fn mkdir(&self, path: &str) -> Result<(), SyncError>;
    async fn stat(&self, path: &str) -> Result<FileInfo, SyncError>;
    async fn exists(&self, path: &str) -> Result<bool, SyncError>;
}

#[derive(Debug, Clone)]
pub struct FileInfo {
    pub path: String,
    pub size: u64,
    pub modified: i64,
    pub hash: Option<String>,
    pub is_dir: bool,
}

#[derive(Debug, Default)]
pub struct UploadResult {
    pub bytes_uploaded: u64,
    pub file_size: u64,
    pub checksum: Option<String>,
    pub elapsed_time: Duration,
}

#[derive(Debug, Default)]
pub struct DownloadResult {
    pub bytes_downloaded: u64,
    pub file_size: u64,
    pub checksum: Option<String>,
    pub elapsed_time: Duration,
}

pub struct RateLimitedProvider<T> {
    inner: T,
    limiter: dyn RateLimiter,
}

impl<T: StorageProvider> RateLimitedProvider<T> {
    pub fn new(inner: T, config: RateLimitConfig) -> RateLimitedProvider<T> {
        Self {
            inner,
            limiter: RateLimiter::new(config),
        }
    }
}

#[async_trait]
impl<T: StorageProvider> StorageProvider for RateLimitedProvider<T> {
    async fn list(&self, path: &str) -> Result<Vec<FileInfo>, SyncError> {
        todo!()
    }

    async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<UploadResult, SyncError> {
        self.limiter.acquire().await;
        self.inner.upload(local_path, remote_path).await
    }

    async fn download(&self, remote_path: &str, local_path: &Path) -> Result<DownloadResult, SyncError> {
        todo!()
    }

    async fn delete(&self, path: &str) -> Result<(), SyncError> {
        todo!()
    }

    async fn mkdir(&self, path: &str) -> Result<(), SyncError> {
        todo!()
    }

    async fn stat(&self, path: &str) -> Result<FileInfo, SyncError> {
        todo!()
    }

    async fn exists(&self, path: &str) -> Result<bool, SyncError> {
        todo!()
    }

    // 其他方法实现...
}