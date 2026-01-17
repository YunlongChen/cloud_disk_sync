use crate::config::AccountConfig;
use crate::error::SyncError;
use crate::providers::{DownloadResult, FileInfo, StorageProvider, UploadResult};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use serde::Deserialize;
use std::path::Path;
use std::time::Duration;

const API_BASE_URL: &str = "https://proapi.115.com";

#[derive(Debug, Deserialize)]
struct FileListResponse {
    state: bool,
    error: Option<String>,
    data: Option<FileListData>,
}

#[derive(Debug, Deserialize)]
struct FileListData {
    data: Vec<FileItem>,
}

#[derive(Debug, Deserialize)]
struct FileItem {
    fid: String,
    cid: String,
    n: String,      // name
    s: Option<u64>, // size
    t: String,      // time (timestamp string)
                    // sha: String,    // hash
}

#[derive(Debug, Deserialize)]
struct BaseResponse {
    state: bool,
    error: Option<String>,
}

pub struct OneOneFiveProvider {
    client: reqwest::Client,
    #[allow(dead_code)]
    cookie: String,
}

impl OneOneFiveProvider {
    pub async fn new(config: &AccountConfig) -> Result<Self, SyncError> {
        let cookie = config
            .credentials
            .get("cookie")
            .ok_or_else(|| {
                SyncError::Config(crate::error::ConfigError::Invalid(
                    "Missing cookie for 115 provider".into(),
                ))
            })?
            .clone();

        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36",
            ),
        );
        headers.insert(
            "Cookie",
            HeaderValue::from_str(&cookie).map_err(|e| {
                SyncError::Config(crate::error::ConfigError::Invalid(format!(
                    "Invalid cookie: {}",
                    e
                )))
            })?,
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| SyncError::Network(e.into()))?;

        Ok(Self { client, cookie })
    }

    async fn get_file_list(&self, cid: &str) -> Result<Vec<FileInfo>, SyncError> {
        let url = format!("{}/open/ufile/files", API_BASE_URL);
        let params = [("foo", "bar"), ("baz", "quux")];
        let resp = self
            .client
            .get(&url)
            .query(&[
                ("aid", "1"),
                ("cid", cid),
                ("limit", "1000"),
                ("show_dir", "1"),
            ])
            .form(&params)
            .send()
            .await
            .map_err(|e| SyncError::Network(e.into()))?;

        let list_resp: FileListResponse = resp
            .json()
            .await
            .map_err(|e| SyncError::Network(e.into()))?;

        if !list_resp.state {
            return Err(SyncError::Provider(crate::error::ProviderError::ApiError(
                list_resp.error.unwrap_or_else(|| "Unknown error".into()),
            )));
        }

        let mut files = Vec::new();
        if let Some(data) = list_resp.data {
            for item in data.data {
                let is_dir = item.fid == item.cid; // Simple check, might need adjustment based on actual API response
                files.push(FileInfo {
                    path: item.n,
                    size: item.s.unwrap_or(0),
                    modified: item.t.parse::<i64>().unwrap_or(0),
                    hash: None, // item.sha is not always present or reliable in list
                    is_dir,
                });
            }
        }

        Ok(files)
    }
}

#[async_trait]
impl StorageProvider for OneOneFiveProvider {
    async fn verify(&self) -> Result<(), SyncError> {
        Ok(())
    }

    async fn list(&self, path: &str) -> Result<Vec<FileInfo>, SyncError> {
        // For simplicity, assuming path is a CID or mapping root to "0"
        let cid = if path == "/" { "0" } else { path };
        self.get_file_list(cid).await
    }

    async fn upload(
        &self,
        _local_path: &Path,
        _remote_path: &str,
    ) -> Result<UploadResult, SyncError> {
        // TODO: Implement actual upload logic (requires complex multi-step process)
        // 1. Pre-upload check (fast upload)
        // 2. Get upload token/server
        // 3. Upload file data
        Err(SyncError::Provider(
            crate::error::ProviderError::NotImplemented("Upload not implemented yet".into()),
        ))
    }

    async fn download(
        &self,
        _remote_path: &str,
        _local_path: &Path,
    ) -> Result<DownloadResult, SyncError> {
        // TODO: Implement download logic (get download url -> download)
        Err(SyncError::Provider(
            crate::error::ProviderError::NotImplemented("Download not implemented yet".into()),
        ))
    }

    async fn delete(&self, path: &str) -> Result<(), SyncError> {
        // API: /rb/delete
        let url = format!("{}/open/ufile/delete", API_BASE_URL);
        let params = [("fid", path)]; // Assuming path is FID for now

        let resp = self
            .client
            .post(&url)
            .form(&params)
            .send()
            .await
            .map_err(|e| SyncError::Network(e.into()))?;

        let base_resp: BaseResponse = resp
            .json()
            .await
            .map_err(|e| SyncError::Network(e.into()))?;

        if !base_resp.state {
            return Err(SyncError::Provider(crate::error::ProviderError::ApiError(
                base_resp.error.unwrap_or_else(|| "Delete failed".into()),
            )));
        }

        Ok(())
    }

    async fn mkdir(&self, _path: &str) -> Result<(), SyncError> {
        // API: /open/folder/add
        // Need parent CID and new folder name. Path parsing needed.
        // For now, assuming path structure is "pid/new_name" is too simple.
        // Usually providers take a full path.
        // Implementing full path to CID resolution is complex without a cache.
        Err(SyncError::Provider(
            crate::error::ProviderError::NotImplemented("Mkdir not implemented yet".into()),
        ))
    }

    async fn stat(&self, _path: &str) -> Result<FileInfo, SyncError> {
        // Use list to find the file/dir
        // This is inefficient but works without dedicated stat API for path
        // For CID, we can query details.
        // Assuming path is a CID for now:
        Err(SyncError::Provider(
            crate::error::ProviderError::NotImplemented("Stat not implemented yet".into()),
        ))
    }

    async fn exists(&self, path: &str) -> Result<bool, SyncError> {
        // Check if file/dir exists via stat or list
        match self.stat(path).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}
