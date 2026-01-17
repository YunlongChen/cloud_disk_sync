use async_trait::async_trait;
use bytes::Bytes;
use cloud_disk_sync::error::{ProviderError, SyncError};
use cloud_disk_sync::providers::{DownloadResult, FileInfo, StorageProvider, UploadResult};
use rand::{rng, Rng};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::sync::Once;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;
use warp::http::Method;
use warp::Filter;

static INIT: Once = Once::new();

pub fn init_logging() {
    INIT.call_once(|| {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .with_test_writer()
            .try_init()
            .ok();
    });
}

#[derive(Clone, Debug)]
pub struct FaultConfig {
    pub latency_ms: u64,
    pub latency_jitter_ms: u64,
    pub error_rate: f64,    // 0.0 to 1.0
    pub error_type: String, // "timeout", "auth", "server"
}

impl Default for FaultConfig {
    fn default() -> Self {
        Self {
            latency_ms: 0,
            latency_jitter_ms: 0,
            error_rate: 0.0,
            error_type: "server".to_string(),
        }
    }
}

pub struct FaultInjectionProvider {
    inner: Box<dyn StorageProvider>,
    config: FaultConfig,
}

impl FaultInjectionProvider {
    pub fn new(inner: Box<dyn StorageProvider>, config: FaultConfig) -> Self {
        Self { inner, config }
    }

    async fn maybe_inject_fault(&self) -> Result<(), SyncError> {
        // 1. 注入延迟
        if self.config.latency_ms > 0 {
            let jitter = if self.config.latency_jitter_ms > 0 {
                rand::rng().random_range(0..=self.config.latency_jitter_ms)
            } else {
                0
            };
            sleep(Duration::from_millis(self.config.latency_ms + jitter)).await;
        }

        // 2. 注入错误
        if self.config.error_rate > 0.0 {
            let r: f64 = rand::random();
            if r < self.config.error_rate {
                return Err(match self.config.error_type.as_str() {
                    "timeout" => {
                        SyncError::Provider(ProviderError::Timeout("Simulated timeout".into()))
                    }
                    "auth" => SyncError::Provider(ProviderError::AuthFailed(
                        "Simulated auth failure".into(),
                    )),
                    _ => SyncError::Provider(ProviderError::ApiError(
                        "Simulated server error".into(),
                    )),
                });
            }
        }
        Ok(())
    }
}

#[async_trait]
impl StorageProvider for FaultInjectionProvider {
    async fn verify(&self) -> Result<(), SyncError> {
        Ok(())
    }

    async fn list(&self, path: &str) -> Result<Vec<FileInfo>, SyncError> {
        self.maybe_inject_fault().await?;
        self.inner.list(path).await
    }

    async fn upload(
        &self,
        local_path: &Path,
        remote_path: &str,
    ) -> Result<UploadResult, SyncError> {
        self.maybe_inject_fault().await?;
        self.inner.upload(local_path, remote_path).await
    }

    async fn download(
        &self,
        remote_path: &str,
        local_path: &Path,
    ) -> Result<DownloadResult, SyncError> {
        self.maybe_inject_fault().await?;
        self.inner.download(remote_path, local_path).await
    }

    async fn delete(&self, path: &str) -> Result<(), SyncError> {
        self.maybe_inject_fault().await?;
        self.inner.delete(path).await
    }

    async fn mkdir(&self, path: &str) -> Result<(), SyncError> {
        self.maybe_inject_fault().await?;
        self.inner.mkdir(path).await
    }

    async fn stat(&self, path: &str) -> Result<FileInfo, SyncError> {
        self.maybe_inject_fault().await?;
        self.inner.stat(path).await
    }

    async fn exists(&self, path: &str) -> Result<bool, SyncError> {
        self.maybe_inject_fault().await?;
        self.inner.exists(path).await
    }
}

// 辅助函数：生成测试文件
pub async fn generate_test_files(dir: &Path, count: usize, size_bytes: usize) -> Vec<String> {
    let mut files = Vec::new();
    if !dir.exists() {
        tokio::fs::create_dir_all(dir).await.unwrap();
    }

    for i in 0..count {
        let filename = format!("test_file_{}.dat", i);
        let path = dir.join(&filename);
        let content: Vec<u8> = (0..size_bytes).map(|_| rand::random::<u8>()).collect();
        tokio::fs::write(&path, content).await.unwrap();
        files.push(filename);
    }
    files
}

// 辅助函数：生成深层目录结构
pub async fn generate_deep_structure(root: &Path, depth: usize, files_per_level: usize) {
    let mut current = root.to_path_buf();
    for d in 0..depth {
        current = current.join(format!("level_{}", d));
        tokio::fs::create_dir_all(&current).await.unwrap();
        generate_test_files(&current, files_per_level, 100).await;
    }
}

// Mock WebDAV Server
#[derive(Debug, Clone)]
pub struct InMemoryFile {
    pub content: Vec<u8>,
    pub is_dir: bool,
}

pub type FileStore = Arc<RwLock<HashMap<String, InMemoryFile>>>;

pub async fn start_mock_server_with_seed(seed: Vec<(&str, &str, bool)>) -> (SocketAddr, FileStore) {
    let store: FileStore = Arc::new(RwLock::new(HashMap::new()));

    {
        let mut files = store.write().await;
        files.insert(
            "/".to_string(),
            InMemoryFile {
                content: vec![],
                is_dir: true,
            },
        );
        files.insert(
            "/file_root".to_string(),
            InMemoryFile {
                content: vec![],
                is_dir: true,
            },
        );
        for (path, content, is_dir) in seed {
            files.insert(
                path.to_string(),
                InMemoryFile {
                    content: if is_dir {
                        vec![]
                    } else {
                        content.as_bytes().to_vec()
                    },
                    is_dir,
                },
            );
        }
    }

    let store_put = store.clone();
    let put_route = warp::put()
        .and(warp::path::full())
        .and(warp::body::bytes())
        .and_then(move |path: warp::path::FullPath, body: Bytes| {
            let store = store_put.clone();
            async move {
                let mut path_str = path.as_str().to_string();
                if path_str.len() > 1 && path_str.ends_with('/') {
                    path_str = path_str.trim_end_matches('/').to_string();
                }
                let mut files = store.write().await;
                files.insert(
                    path_str.clone(),
                    InMemoryFile {
                        content: body.to_vec(),
                        is_dir: false,
                    },
                );

                // Ensure parent directories exist
                let path = std::path::Path::new(&path_str);
                if let Some(parent) = path.parent() {
                    let parent_str = parent.to_string_lossy().replace("\\", "/");
                    if !parent_str.is_empty() && parent_str != "/" {
                        if !files.contains_key(&parent_str) {
                            files.insert(
                                parent_str,
                                InMemoryFile {
                                    content: vec![],
                                    is_dir: true,
                                },
                            );
                        }
                    }
                }

                Ok::<_, warp::Rejection>(warp::reply::with_status(
                    String::new(),
                    warp::http::StatusCode::CREATED,
                ))
            }
        });

    let store_get = store.clone();
    let get_route =
        warp::get()
            .and(warp::path::full())
            .and_then(move |path: warp::path::FullPath| {
                let store = store_get.clone();
                async move {
                    let mut path_str = path.as_str().to_string();
                    if path_str.len() > 1 && path_str.ends_with('/') {
                        path_str = path_str.trim_end_matches('/').to_string();
                    }
                    let files = store.read().await;
                    if let Some(file) = files.get(&path_str) {
                        if !file.is_dir {
                            return Ok::<_, warp::Rejection>(warp::reply::with_status(
                                file.content.clone(),
                                warp::http::StatusCode::OK,
                            ));
                        }
                    }
                    Ok::<_, warp::Rejection>(warp::reply::with_status(
                        Vec::<u8>::new(),
                        warp::http::StatusCode::NOT_FOUND,
                    ))
                }
            });

    let store_delete = store.clone();
    let delete_route =
        warp::delete()
            .and(warp::path::full())
            .and_then(move |path: warp::path::FullPath| {
                let store = store_delete.clone();
                async move {
                    let mut path_str = path.as_str().to_string();
                    if path_str.len() > 1 && path_str.ends_with('/') {
                        path_str = path_str.trim_end_matches('/').to_string();
                    }
                    let mut files = store.write().await;
                    if files.remove(&path_str).is_some() {
                        Ok::<_, warp::Rejection>(warp::reply::with_status(
                            String::new(),
                            warp::http::StatusCode::OK,
                        ))
                    } else {
                        Ok::<_, warp::Rejection>(warp::reply::with_status(
                            String::new(),
                            warp::http::StatusCode::NOT_FOUND,
                        ))
                    }
                }
            });

    let store_prop = store.clone();
    let propfind_route = warp::method()
        .and(warp::path::full())
        .and(warp::header::optional("Depth"))
        .and(warp::body::bytes())
        .and_then(
            move |method: Method,
                  path: warp::path::FullPath,
                  _depth: Option<String>,
                  _body: Bytes| {
                let store = store_prop.clone();
                async move {
                    let path_str = path.as_str().to_string();
                    // Normalize path: remove trailing slash unless it's root
                    let path_str = if path_str.len() > 1 && path_str.ends_with('/') {
                        path_str.trim_end_matches('/').to_string()
                    } else {
                        path_str
                    };

                    if method == Method::from_bytes(b"MKCOL").unwrap() {
                        let mut files = store.write().await;
                        if files.contains_key(&path_str) {
                            // Already exists
                            return Ok::<_, warp::Rejection>(warp::reply::with_status(
                                String::new(),
                                warp::http::StatusCode::METHOD_NOT_ALLOWED,
                            ));
                        }
                        files.insert(
                            path_str,
                            InMemoryFile {
                                content: vec![],
                                is_dir: true,
                            },
                        );
                        return Ok(warp::reply::with_status(
                            String::new(),
                            warp::http::StatusCode::CREATED,
                        ));
                    }

                    if method != Method::from_bytes(b"PROPFIND").unwrap() {
                        return Err(warp::reject::not_found());
                    }
                    let files = store.read().await;
                    if let Some(entry) = files.get(&path_str) {
                        let mut xml = String::from(
                            r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:">
"#,
                        );
                        // 返回自身
                        let self_href = if entry.is_dir {
                            format!("{}/", path_str)
                        } else {
                            path_str.clone()
                        };
                        // Make sure href always has a leading slash
                        let self_href_path = if !self_href.starts_with('/') {
                            format!("/{}", self_href)
                        } else {
                            self_href
                        };

                        // Remove double slash if present (e.g. //file.txt)
                        let self_href_path = self_href_path.replace("//", "/");

                        xml.push_str(&format!(
                            "<d:response>\n  <d:href>{}</d:href>\n  <d:propstat>\n    <d:prop>\n      <d:getcontentlength>{}</d:getcontentlength>\n      <d:getlastmodified>Thu, 01 Jan 1970 00:00:00 GMT</d:getlastmodified>\n      <d:resourcetype>{}</d:resourcetype>\n    </d:prop>\n    <d:status>HTTP/1.1 200 OK</d:status>\n  </d:propstat>\n</d:response>\n",
                            self_href_path,
                            entry.content.len(),
                            if entry.is_dir { "<d:collection/>" } else { "" }
                        ));
                        // 如果是目录，列出直接子项
                        if entry.is_dir {
                            let depth = _depth.as_deref().unwrap_or("1");
                            if depth != "0" {
                                let prefix = if path_str == "/" {
                                    "/".to_string()
                                } else {
                                    format!("{}/", path_str)
                                };

                                for (p, f) in files.iter() {
                                    if p.starts_with(&prefix) && p != &path_str {
                                        let rel = p.strip_prefix(&prefix).unwrap();
                                        let is_direct_child = if depth == "infinity" {
                                            true
                                        } else {
                                            !rel.trim_end_matches('/').contains('/')
                                        };

                                        if is_direct_child {
                                            let href = if f.is_dir {
                                                format!("{}/", p)
                                            } else {
                                                p.clone()
                                            };
                                            // Make sure href always has a leading slash if p doesn't (though p usually does)
                                            let href_path = if !href.starts_with('/') {
                                                format!("/{}", href)
                                            } else {
                                                href
                                            };

                                            // Remove double slash if present (e.g. //file.txt)
                                            let href_path = href_path.replace("//", "/");

                                            xml.push_str(&format!(
                                                "<d:response>\n  <d:href>{}</d:href>\n  <d:propstat>\n    <d:prop>\n      <d:getcontentlength>{}</d:getcontentlength>\n      <d:getlastmodified>Thu, 01 Jan 1970 00:00:00 GMT</d:getlastmodified>\n      <d:resourcetype>{}</d:resourcetype>\n    </d:prop>\n    <d:status>HTTP/1.1 200 OK</d:status>\n  </d:propstat>\n</d:response>\n",
                                                href_path,
                                                f.content.len(),
                                                if f.is_dir { "<d:collection/>" } else { "" }
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                        xml.push_str("</d:multistatus>\n");
                        Ok::<_, warp::Rejection>(warp::reply::with_status(
                            xml,
                            warp::http::StatusCode::MULTI_STATUS,
                        ))
                    } else {
                        Ok::<_, warp::Rejection>(warp::reply::with_status(
                            String::new(),
                            warp::http::StatusCode::NOT_FOUND,
                        ))
                    }
                }
            },
        );

    let routes = put_route.or(get_route).or(delete_route).or(propfind_route);
    // 这里绑定到端口0的作用是让系统分配
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    // 使用正确的 warp 服务器启动方式，兼容所有平台
    // 注意：warp 0.4 需要 SocketAddr，而不是 TcpListener
    let server_future = warp::serve(routes).run(addr);
    tokio::spawn(server_future);

    (addr, store)
}
