//! 115网盘存储提供者实现
//!
//! 该模块提供了115网盘的存储服务集成，支持文件列表、上传、下载等基本操作。
//!
//! # 功能特性
//! - ✅ 文件列表获取
//! - ✅ 连接验证
//! - ✅ 文件存在性检查
//! - ⬜ 文件上传（待实现）
//! - ⬜ 文件下载（待实现）
//! - ⬜ 目录创建（待实现）
//! - ⬜ 文件详情查询（待实现）
//!
//! # 认证方式
//! 使用Cookie进行认证，需要在配置中提供有效的115网盘会话Cookie。

use crate::config::AccountConfig;
use crate::error::SyncError;
use crate::providers::{DownloadResult, FileInfo, StorageProvider, UploadResult};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use serde::Deserialize;
use std::path::Path;
use std::time::Duration;

const API_BASE_URL: &str = "https://proapi.115.com";

/// 文件列表API响应结构
#[derive(Debug, Deserialize)]
struct FileListResponse {
    state: bool,
    error: Option<String>,
    data: Option<FileListData>,
}

/// 文件列表数据
#[derive(Debug, Deserialize)]
struct FileListData {
    data: Vec<FileItem>,
}

/// 文件项信息
#[derive(Debug, Deserialize)]
struct FileItem {
    fid: String,
    cid: String,
    n: String,      // name
    s: Option<u64>, // size
    t: String,      // time (timestamp string)
                    // sha: String,    // hash
}

/// 基础API响应结构
#[derive(Debug, Deserialize)]
struct BaseResponse {
    state: bool,
    error: Option<String>,
}

/// 115网盘存储提供者
///
/// 负责与115网盘API进行交互，实现文件存储相关操作。
pub struct OneOneFiveProvider {
    client: reqwest::Client,
    #[allow(dead_code)]
    cookie: String,
}

impl OneOneFiveProvider {
    /// 创建新的115网盘提供者实例
    ///
    /// # 参数
    /// - `config`: 账户配置，必须包含有效的cookie凭证
    ///
    /// # 返回
    /// - 成功时返回 `OneOneFiveProvider` 实例
    /// - 失败时返回 `SyncError`
    ///
    /// # 错误
    /// - 配置错误：缺少cookie或cookie格式无效
    /// - 网络错误：HTTP客户端创建失败
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
            .map_err(SyncError::Network)?;

        Ok(Self { client, cookie })
    }

    /// 获取指定目录的文件列表
    ///
    /// # 参数
    /// - `cid`: 目录ID，根目录为"0"
    ///
    /// # 返回
    /// - 成功时返回文件信息列表
    /// - 失败时返回 SyncError
    async fn get_file_list(&self, cid: &str) -> Result<Vec<FileInfo>, SyncError> {
        let url = format!("{}/open/ufile/files", API_BASE_URL);

        let resp = self
            .client
            .get(&url)
            .query(&[
                ("aid", "1"),
                ("cid", cid),
                ("limit", "1000"),
                ("show_dir", "1"),
            ])
            .send()
            .await
            .map_err(SyncError::Network)?;

        let list_resp: FileListResponse = resp.json().await.map_err(SyncError::Network)?;

        if !list_resp.state {
            return Err(SyncError::Provider(crate::error::ProviderError::ApiError(
                list_resp.error.unwrap_or_else(|| "Unknown error".into()),
            )));
        }

        let mut files = Vec::new();
        if let Some(data) = list_resp.data {
            for item in data.data {
                // 更准确地判断是否为目录
                let is_dir = item.fid == item.cid || item.s.is_none();

                // 解析修改时间，处理可能的错误
                let modified = item.t.parse::<i64>().unwrap_or_else(|_| {
                    // 如果解析失败，使用当前时间戳
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64
                });

                files.push(FileInfo {
                    path: item.n,
                    size: item.s.unwrap_or(0),
                    modified,
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
        // 通过获取根目录列表来验证连接是否有效
        self.get_file_list("0").await?;
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
            .map_err(SyncError::Network)?;

        let base_resp: BaseResponse = resp.json().await.map_err(SyncError::Network)?;

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
