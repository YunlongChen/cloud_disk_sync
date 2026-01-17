use crate::config::AccountConfig;
use crate::core::rate_limit::SlidingWindowRateLimiter;
use crate::core::traits::RateLimiter;
use crate::error::{ProviderError, SyncError};
use crate::providers::{DownloadResult, FileInfo, StorageProvider, UploadResult};
use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

// 阿里云盘实现
pub struct AliYunDriveProvider {
    client: reqwest::Client,
    token: String,
    refresh_token: String,
    rate_limiter: Arc<dyn RateLimiter>,
}

impl AliYunDriveProvider {
    pub async fn new(config: &AccountConfig) -> Result<Self, ProviderError> {
        let token = config.credentials.get("token");

        match token {
            None => Err(ProviderError::NotFound("参数异常".into())),
            Some(token_value) => Ok(Self {
                client: reqwest::Client::new(),
                token: token_value.clone(),
                refresh_token: config
                    .credentials
                    .get("refresh_token")
                    .cloned()
                    .unwrap_or_default(),
                rate_limiter: Arc::new(SlidingWindowRateLimiter {
                    window_size: Duration::from_secs(1),
                    max_requests: 1u64,
                    requests: Mutex::new(vec![]),
                }),
            }),
        }
    }

    async fn refresh_token_if_needed(&mut self) -> Result<(), ProviderError> {
        // 实现token刷新逻辑
        Ok(())
    }
}

#[async_trait]
impl StorageProvider for AliYunDriveProvider {
    async fn verify(&self) -> Result<(), SyncError> {
        todo!()
    }

    async fn list(&self, path: &str) -> Result<Vec<FileInfo>, SyncError> {
        Ok(vec![FileInfo {
            path: path.to_string(),
            size: 0,
            modified: 0,
            hash: None,
            is_dir: true,
        }])
    }
    async fn upload(
        &self,
        local_path: &Path,
        remote_path: &str,
    ) -> Result<UploadResult, SyncError> {
        // 实现阿里云盘上传逻辑
        // 支持分片上传、断点续传
        Ok(UploadResult::default())
    }

    async fn download(
        &self,
        remote_path: &str,
        local_path: &Path,
    ) -> Result<DownloadResult, SyncError> {
        // 实现下载逻辑
        Ok(DownloadResult::default())
    }

    async fn delete(&self, path: &str) -> Result<(), SyncError> {
        Ok(())
    }
    async fn mkdir(&self, path: &str) -> Result<(), SyncError> {
        Ok(())
    }
    async fn stat(&self, path: &str) -> Result<FileInfo, SyncError> {
        Ok(FileInfo {
            path: path.to_string(),
            size: 0,
            modified: 0,
            hash: None,
            is_dir: true,
        })
    }
    async fn exists(&self, path: &str) -> Result<bool, SyncError> {
        Ok(true)
    }
}
