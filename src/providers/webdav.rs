use crate::config::AccountConfig;
use crate::error::{ProviderError, SyncError};
use crate::providers::{DownloadResult, FileInfo, StorageProvider, UploadResult};
use async_trait::async_trait;
use base64::Engine;
use reqwest::{Client, Method, StatusCode, Url};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, error, info, instrument, warn};

/// WebDAV 存储提供商
pub struct WebDavProvider {
    client: Client,
    base_url: String,
    path_prefix: String,
    username: String,
    password: String,
}

impl WebDavProvider {
    /// 创建新的 WebDAV 提供商
    #[instrument(skip(config), fields(account_id = %config.id, account_name = %config.name))]
    pub async fn new(config: &AccountConfig) -> Result<Self, ProviderError> {
        info!("初始化 WebDAV Provider");

        let url = config.credentials.get("url").ok_or_else(|| {
            error!("配置缺少 URL");
            ProviderError::MissingCredentials("url".to_string())
        })?;

        let username = config.credentials.get("username").ok_or_else(|| {
            error!("配置缺少用户名");
            ProviderError::MissingCredentials("username".to_string())
        })?;

        let password = config.credentials.get("password").ok_or_else(|| {
            error!("配置缺少密码");
            ProviderError::MissingCredentials("password".to_string())
        })?;

        debug!(url = %url, username = %username, "解析 WebDAV 凭证");

        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| {
                error!(error = %e, "创建 HTTP 客户端失败");
                ProviderError::ConnectionFailed(e.to_string())
            })?;

        let parsed_url = Url::parse(url).map_err(|e| {
            error!(error = %e, "URL 解析失败");
            ProviderError::ConnectionFailed(format!("Invalid URL: {}", e))
        })?;

        let path_prefix = urlencoding::decode(parsed_url.path())
            .unwrap_or(std::borrow::Cow::Borrowed(parsed_url.path()))
            .trim_end_matches('/')
            .to_string();

        info!(base_url = %url, path_prefix = %path_prefix, "WebDAV Provider 初始化成功");

        Ok(Self {
            client,
            base_url: url.trim_end_matches('/').to_string(),
            path_prefix,
            username: username.clone(),
            password: password.clone(),
        })
    }

    /// 获取完整的 URL
    fn get_full_url(&self, path: &str) -> String {
        let path = path.trim_start_matches('/');

        // Split path into components and encode each one
        let encoded_path: Vec<String> = path
            .split('/')
            .map(|component| urlencoding::encode(component).to_string())
            .collect();

        let encoded_path_str = encoded_path.join("/");

        let url = format!("{}/{}", self.base_url, encoded_path_str);
        debug!(path = %path, url = %url, "构建完整 URL");
        url
    }

    /// 创建基本认证头
    fn create_auth_header(&self) -> String {
        let credentials = format!("{}:{}", self.username, self.password);
        let encoded = base64::engine::general_purpose::STANDARD.encode(credentials);
        format!("Basic {}", encoded)
    }

    /// 解析 WebDAV PROPFIND 响应
    #[instrument(skip(self, xml), fields(base_path = %base_path))]
    fn parse_propfind_response(
        &self,
        xml: &str,
        base_path: &str,
    ) -> Result<Vec<FileInfo>, SyncError> {
        debug!("开始解析 PROPFIND 响应");
        use quick_xml::events::Event;
        use quick_xml::reader::Reader;

        let mut files = Vec::new();
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);

        let mut buf = Vec::new();

        let mut current_path: Option<String> = None;
        let mut current_size: u64 = 0;
        let mut is_collection = false;

        // 状态标记
        let mut in_response = false;
        let mut in_href = false;
        let mut in_prop = false;
        let mut in_getcontentlength = false;
        let mut in_resourcetype = false;
        let _in_collection = false;

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let name = e.name();
                    let name_str = String::from_utf8_lossy(name.as_ref()).to_lowercase();

                    if name_str.ends_with("response") {
                        in_response = true;
                        current_path = None;
                        current_size = 0;
                        is_collection = false;
                    } else if in_response {
                        if name_str.ends_with("href") {
                            in_href = true;
                        } else if name_str.ends_with("prop") {
                            in_prop = true;
                        } else if in_prop {
                            if name_str.ends_with("getcontentlength") {
                                in_getcontentlength = true;
                            } else if name_str.ends_with("resourcetype") {
                                in_resourcetype = true;
                            } else if in_resourcetype && name_str.ends_with("collection") {
                                is_collection = true;
                            }
                        }
                    }
                }
                Ok(Event::Empty(ref e)) => {
                    let name = e.name();
                    let name_str = String::from_utf8_lossy(name.as_ref()).to_lowercase();
                    if in_resourcetype && name_str.ends_with("collection") {
                        is_collection = true;
                    }
                }
                Ok(Event::Text(e)) => {
                    if in_href {
                        // Workaround for unescape compilation error: use raw string conversion
                        // This assumes standard URLs without complex XML entities needing unescape
                        let href = String::from_utf8_lossy(e.as_ref()).to_string();

                        // Decode URL encoding
                        let decoded_href =
                            urlencoding::decode(&href).unwrap_or(std::borrow::Cow::Borrowed(&href));
                        let mut path = decoded_href.to_string();

                        if path.starts_with(&self.base_url) {
                            path = path.trim_start_matches(&self.base_url).to_string();
                        } else if path.starts_with(&self.path_prefix) {
                            path = path.trim_start_matches(&self.path_prefix).to_string();
                        }

                        // 确保路径以 / 开头（如果是根目录下的文件）
                        if !path.starts_with('/') && !path.is_empty() {
                            path = format!("/{}", path);
                        }

                        // Handle cases where the path might be just "/" after trimming
                        if path.is_empty() {
                            path = "/".to_string();
                        }

                        current_path = Some(path);
                    } else if in_getcontentlength {
                        let size_str = String::from_utf8_lossy(e.as_ref()).to_string();
                        if let Ok(size) = size_str.parse::<u64>() {
                            current_size = size;
                        }
                    }
                }
                Ok(Event::End(ref e)) => {
                    let name = e.name();
                    let name_str = String::from_utf8_lossy(name.as_ref()).to_lowercase();

                    if name_str.ends_with("response") {
                        if let Some(path) = current_path.take() {
                            // 跳过基础路径本身
                            // Normalize paths for comparison (remove trailing slashes)
                            let norm_path = path.trim_end_matches('/');
                            let norm_base = base_path.trim_end_matches('/');

                            // Debug logging to help trace path issues
                            debug!(path = %path, norm_path = %norm_path, base = %base_path, norm_base = %norm_base, "Checking if path is base path");

                            if norm_path != norm_base && !path.is_empty() {
                                files.push(FileInfo {
                                    path, // Keep original path (maybe with trailing slash for dirs)
                                    size: current_size,
                                    modified: SystemTime::now()
                                        .duration_since(UNIX_EPOCH)
                                        .unwrap()
                                        .as_secs()
                                        as i64,
                                    hash: None,
                                    is_dir: is_collection,
                                });
                            }
                        }
                        in_response = false;
                    } else if name_str.ends_with("href") {
                        in_href = false;
                    } else if name_str.ends_with("prop") {
                        in_prop = false;
                    } else if name_str.ends_with("getcontentlength") {
                        in_getcontentlength = false;
                    } else if name_str.ends_with("resourcetype") {
                        in_resourcetype = false;
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    error!("Error parsing XML: {:?}", e);
                    break;
                }
                _ => {}
            }
            buf.clear();
        }

        info!(
            count = files.len(),
            "解析完成，共 {} 个文件/目录",
            files.len()
        );
        Ok(files)
    }
}

#[async_trait]
impl StorageProvider for WebDavProvider {
    async fn verify(&self) -> Result<(), SyncError> {
        Ok(())
    }

    /// 列出目录内容
    async fn list(&self, path: &str) -> Result<Vec<FileInfo>, SyncError> {
        let url = self.get_full_url(path);

        let response = self
            .client
            .request(Method::from_bytes(b"PROPFIND").unwrap(), &url)
            .header("Authorization", self.create_auth_header())
            .header("Depth", "1")
            .header("Content-Type", "application/xml")
            .body(
                r#"<?xml version="1.0" encoding="utf-8"?>
                <d:propfind xmlns:d="DAV:">
                    <d:prop>
                        <d:displayname/>
                        <d:getcontentlength/>
                        <d:getlastmodified/>
                        <d:resourcetype/>
                    </d:prop>
                </d:propfind>"#,
            )
            .send()
            .await
            .map_err(|e| SyncError::Network(e))?;

        if !response.status().is_success() {
            return Err(SyncError::Provider(ProviderError::ApiError(format!(
                "PROPFIND failed: {}",
                response.status()
            ))));
        }

        let body = response.text().await.map_err(|e| SyncError::Network(e))?;

        debug!("PROPFIND Response Body: {}", body);

        self.parse_propfind_response(&body, path)
    }

    /// 上传文件
    async fn upload(
        &self,
        local_path: &Path,
        remote_path: &str,
    ) -> Result<UploadResult, SyncError> {
        let url = self.get_full_url(remote_path);
        let start_time = SystemTime::now();

        // 读取文件内容
        let file_data = tokio::fs::read(local_path)
            .await
            .map_err(|e| SyncError::Io(e))?;

        let file_size = file_data.len() as u64;

        // 上传文件
        let response = self
            .client
            .put(&url)
            .header("Authorization", self.create_auth_header())
            .body(file_data)
            .send()
            .await
            .map_err(|e| SyncError::Network(e))?;

        if !response.status().is_success() {
            return Err(SyncError::Provider(ProviderError::ApiError(format!(
                "Upload failed: {}",
                response.status()
            ))));
        }

        let elapsed = SystemTime::now()
            .duration_since(start_time)
            .unwrap_or(Duration::from_secs(0));

        Ok(UploadResult {
            bytes_uploaded: file_size,
            file_size,
            checksum: None,
            elapsed_time: elapsed,
        })
    }

    /// 下载文件
    #[instrument(skip(self), fields(remote_path = %remote_path, local_path = %local_path.display()))]
    async fn download(
        &self,
        remote_path: &str,
        local_path: &Path,
    ) -> Result<DownloadResult, SyncError> {
        info!("开始下载文件");
        let url = self.get_full_url(remote_path);
        let start_time = SystemTime::now();

        debug!("发送 GET 请求");
        let response = self
            .client
            .get(&url)
            .header("Authorization", self.create_auth_header())
            .send()
            .await
            .map_err(|e| {
                error!(error = %e, "下载请求失败");
                SyncError::Network(e)
            })?;

        let status = response.status();
        debug!(status = %status, "收到下载响应");

        if !status.is_success() {
            warn!(status = %status, "文件不存在或下载失败");
            return Err(SyncError::Provider(ProviderError::FileNotFound(
                remote_path.to_string(),
            )));
        }

        let bytes = response.bytes().await.map_err(|e| {
            error!(error = %e, "读取响应数据失败");
            SyncError::Network(e)
        })?;

        let file_size = bytes.len() as u64;
        debug!(file_size = %file_size, "下载数据大小: {} 字节", file_size);

        // 确保父目录存在
        if let Some(parent) = local_path.parent() {
            debug!(parent = %parent.display(), "创建父目录");
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                error!(error = %e, "创建父目录失败");
                SyncError::Io(e)
            })?;
        }

        // 写入文件
        debug!("写入本地文件");
        tokio::fs::write(local_path, bytes).await.map_err(|e| {
            error!(error = %e, "写入本地文件失败");
            SyncError::Io(e)
        })?;

        let elapsed = SystemTime::now()
            .duration_since(start_time)
            .unwrap_or(Duration::from_secs(0));

        let speed = if elapsed.as_secs() > 0 {
            file_size as f64 / elapsed.as_secs_f64() / 1024.0 / 1024.0
        } else {
            0.0
        };

        info!(
            file_size = %file_size,
            elapsed_ms = elapsed.as_millis(),
            speed_mbps = %format!("{:.2}", speed),
            "文件下载成功: {} 字节，耗时 {} ms，速度 {:.2} MB/s",
            file_size, elapsed.as_millis(), speed
        );

        Ok(DownloadResult {
            bytes_downloaded: file_size,
            file_size,
            checksum: None,
            elapsed_time: elapsed,
        })
    }

    /// 删除文件或目录
    #[instrument(skip(self), fields(path = %path))]
    async fn delete(&self, path: &str) -> Result<(), SyncError> {
        info!("开始删除文件或目录");
        let url = self.get_full_url(path);

        debug!("发送 DELETE 请求");
        let response = self
            .client
            .delete(&url)
            .header("Authorization", self.create_auth_header())
            .send()
            .await
            .map_err(|e| {
                error!(error = %e, "删除请求失败");
                SyncError::Network(e)
            })?;

        let status = response.status();
        debug!(status = %status, "收到删除响应");

        if status.is_success() {
            info!("删除成功");
            Ok(())
        } else if status == StatusCode::NOT_FOUND {
            warn!("文件或目录不存在，视为删除成功");
            Ok(())
        } else {
            error!(status = %status, "删除失败");
            Err(SyncError::Provider(ProviderError::ApiError(format!(
                "Delete failed: {}",
                status
            ))))
        }
    }

    /// 创建目录
    #[instrument(skip(self), fields(path = %path))]
    async fn mkdir(&self, path: &str) -> Result<(), SyncError> {
        info!("开始创建目录");
        let url = self.get_full_url(path);

        debug!("发送 MKCOL 请求");
        let response = self
            .client
            .request(Method::from_bytes(b"MKCOL").unwrap(), &url)
            .header("Authorization", self.create_auth_header())
            .send()
            .await
            .map_err(|e| {
                error!(error = %e, "创建目录请求失败");
                SyncError::Network(e)
            })?;

        let status = response.status();
        debug!(status = %status, "收到 MKCOL 响应");

        if status.is_success() {
            info!("目录创建成功");
            Ok(())
        } else if status == StatusCode::METHOD_NOT_ALLOWED {
            // METHOD_NOT_ALLOWED 可能表示目录已存在
            warn!("目录可能已存在，视为创建成功");
            Ok(())
        } else {
            error!(status = %status, "创建目录失败");
            Err(SyncError::Provider(ProviderError::ApiError(format!(
                "MKCOL failed: {}",
                status
            ))))
        }
    }

    /// 获取文件或目录信息
    #[instrument(skip(self), fields(path = %path))]
    async fn stat(&self, path: &str) -> Result<FileInfo, SyncError> {
        debug!("查询文件或目录信息");
        let url = self.get_full_url(path);

        let response = self
            .client
            .request(Method::from_bytes(b"PROPFIND").unwrap(), &url)
            .header("Authorization", self.create_auth_header())
            .header("Depth", "0")
            .send()
            .await
            .map_err(|e| {
                error!(error = %e, "PROPFIND 请求失败");
                SyncError::Network(e)
            })?;

        let status = response.status();
        debug!(status = %status, "收到 stat 响应");

        if !status.is_success() {
            warn!("文件或目录不存在");
            return Err(SyncError::Provider(ProviderError::FileNotFound(
                path.to_string(),
            )));
        }

        let is_dir = path.ends_with('/');
        debug!(is_dir = %is_dir, "查询成功");

        Ok(FileInfo {
            path: path.to_string(),
            size: 0,
            modified: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            hash: None,
            is_dir,
        })
    }

    /// 检查文件或目录是否存在
    #[instrument(skip(self), fields(path = %path))]
    async fn exists(&self, path: &str) -> Result<bool, SyncError> {
        debug!("检查文件或目录是否存在");
        match self.stat(path).await {
            Ok(_) => {
                debug!("文件或目录存在");
                Ok(true)
            }
            Err(SyncError::Provider(ProviderError::FileNotFound(_))) => {
                debug!("文件或目录不存在");
                Ok(false)
            }
            Err(e) => {
                warn!(error = %e, "检查存在性时发生错误");
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_get_full_url() {
        let config = AccountConfig {
            id: "test".to_string(),
            provider: crate::config::ProviderType::WebDAV,
            name: "test".to_string(),
            credentials: {
                let mut creds = HashMap::new();
                creds.insert("url".to_string(), "http://localhost:8080/dav".to_string());
                creds.insert("username".to_string(), "user".to_string());
                creds.insert("password".to_string(), "pass".to_string());
                creds
            },
            rate_limit: None,
            retry_policy: crate::config::RetryPolicy::default(),
        };

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let provider = runtime.block_on(WebDavProvider::new(&config)).unwrap();

        assert_eq!(
            provider.get_full_url("/test/file.txt"),
            "http://localhost:8080/dav/test/file.txt"
        );
        assert_eq!(
            provider.get_full_url("test/file.txt"),
            "http://localhost:8080/dav/test/file.txt"
        );

        // Test URL encoding
        assert_eq!(
            provider.get_full_url("/test/file with spaces.txt"),
            "http://localhost:8080/dav/test/file%20with%20spaces.txt"
        );
        assert_eq!(
            provider.get_full_url("/test/special{}.txt"),
            "http://localhost:8080/dav/test/special%7B%7D.txt"
        );
    }

    #[test]
    fn test_auth_header() {
        let config = AccountConfig {
            id: "test".to_string(),
            provider: crate::config::ProviderType::WebDAV,
            name: "test".to_string(),
            credentials: {
                let mut creds = HashMap::new();
                creds.insert("url".to_string(), "http://localhost:8080".to_string());
                creds.insert("username".to_string(), "testuser".to_string());
                creds.insert("password".to_string(), "testpass".to_string());
                creds
            },
            rate_limit: None,
            retry_policy: crate::config::RetryPolicy::default(),
        };

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let provider = runtime.block_on(WebDavProvider::new(&config)).unwrap();

        let auth = provider.create_auth_header();
        assert!(auth.starts_with("Basic "));

        // 验证 base64 编码是否正确
        let encoded = auth.strip_prefix("Basic ").unwrap();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), "testuser:testpass");
    }

    // 功能测试：使用模拟服务器测试实际操作
    #[cfg(test)]
    mod integration {
        use super::*;
        use std::env;
        use std::net::SocketAddr;
        use std::sync::Arc;
        use tokio::sync::RwLock;

        #[derive(Debug, Clone)]
        struct InMemoryFile {
            content: Vec<u8>,
            is_dir: bool,
        }

        type FileStore = Arc<RwLock<HashMap<String, InMemoryFile>>>;

        async fn start_mock_server() -> (SocketAddr, FileStore) {
            use warp::Filter;

            let store: FileStore = Arc::new(RwLock::new(HashMap::new()));

            // 初始化根目录
            {
                let mut files = store.write().await;
                files.insert(
                    "/".to_string(),
                    InMemoryFile {
                        content: vec![],
                        is_dir: true,
                    },
                );
            }

            let store_clone = store.clone();

            // PUT 处理器（上传）
            let put_route = warp::put()
                .and(warp::path::full())
                .and(warp::body::bytes())
                .and_then({
                    let store = store_clone.clone();
                    move |path: warp::path::FullPath, body: bytes::Bytes| {
                        let store = store.clone();
                        async move {
                            let path_str = path.as_str().to_string();
                            let mut files = store.write().await;

                            files.insert(
                                path_str,
                                InMemoryFile {
                                    content: body.to_vec(),
                                    is_dir: false,
                                },
                            );

                            Ok::<_, warp::Rejection>(warp::reply::with_status(
                                String::new(),
                                warp::http::StatusCode::CREATED,
                            ))
                        }
                    }
                });

            // GET 处理器（下载）
            let get_route = warp::get().and(warp::path::full()).and_then({
                let store = store_clone.clone();
                move |path: warp::path::FullPath| {
                    let store = store.clone();
                    async move {
                        let path_str = path.as_str();
                        let files = store.read().await;

                        if let Some(file) = files.get(path_str) {
                            if !file.is_dir {
                                return Ok::<_, warp::Rejection>(warp::reply::with_status(
                                    file.content.clone(),
                                    warp::http::StatusCode::OK,
                                ));
                            }
                        }

                        Ok(warp::reply::with_status(
                            vec![],
                            warp::http::StatusCode::NOT_FOUND,
                        ))
                    }
                }
            });

            let routes = put_route.or(get_route);
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let server_future = warp::serve(routes).run(addr);
            tokio::spawn(server_future);

            (addr, store)
        }

        #[tokio::test]
        async fn test_upload_download() {
            let (addr, _store) = start_mock_server().await;

            let config = AccountConfig {
                id: "test".to_string(),
                provider: crate::config::ProviderType::WebDAV,
                name: "test".to_string(),
                credentials: {
                    let mut creds = HashMap::new();
                    creds.insert("url".to_string(), format!("http://{}", addr));
                    creds.insert("username".to_string(), "test".to_string());
                    creds.insert("password".to_string(), "test".to_string());
                    creds
                },
                rate_limit: None,
                retry_policy: crate::config::RetryPolicy::default(),
            };

            let provider = WebDavProvider::new(&config).await.unwrap();

            // 创建测试文件
            let temp_dir = env::temp_dir();
            let test_file = temp_dir.join("webdav_test_upload.txt");
            let test_content = b"Hello WebDAV";
            tokio::fs::write(&test_file, test_content).await.unwrap();

            // 上传
            let upload_result = provider.upload(&test_file, "/test.txt").await.unwrap();
            assert_eq!(upload_result.file_size, test_content.len() as u64);

            // 下载
            let download_file = temp_dir.join("webdav_test_download.txt");
            let download_result = provider
                .download("/test.txt", &download_file)
                .await
                .unwrap();
            assert_eq!(download_result.file_size, test_content.len() as u64);

            // 验证内容
            let downloaded = tokio::fs::read(&download_file).await.unwrap();
            assert_eq!(&downloaded, test_content);

            // 清理
            tokio::fs::remove_file(&test_file).await.ok();
            tokio::fs::remove_file(&download_file).await.ok();
        }

        #[tokio::test]
        async fn test_large_file() {
            let (addr, _store) = start_mock_server().await;

            let config = AccountConfig {
                id: "test".to_string(),
                provider: crate::config::ProviderType::WebDAV,
                name: "test".to_string(),
                credentials: {
                    let mut creds = HashMap::new();
                    creds.insert("url".to_string(), format!("http://{}", addr));
                    creds.insert("username".to_string(), "test".to_string());
                    creds.insert("password".to_string(), "test".to_string());
                    creds
                },
                rate_limit: None,
                retry_policy: crate::config::RetryPolicy::default(),
            };

            let provider = WebDavProvider::new(&config).await.unwrap();

            // 创建 1MB 测试文件
            let temp_dir = env::temp_dir();
            let test_file = temp_dir.join("webdav_test_large.bin");
            let large_content = vec![0u8; 1024 * 1024];
            tokio::fs::write(&test_file, &large_content).await.unwrap();

            // 上传大文件
            let upload_result = provider.upload(&test_file, "/large.bin").await.unwrap();
            assert_eq!(upload_result.file_size, large_content.len() as u64);

            // 清理
            tokio::fs::remove_file(&test_file).await.ok();
        }
    }
}
