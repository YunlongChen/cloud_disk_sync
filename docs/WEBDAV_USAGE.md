# WebDAV Provider 使用示例

本文档展示如何在实际项目中使用 WebDAV Provider。

## 快速开始

### 1. 添加 WebDAV 账户

```bash
# 使用 CLI 添加 WebDAV 账户
cloud-disk-sync add-account \
  --name my-webdav \
  --provider webdav

# 按提示输入：
# - WebDAV 服务器地址：https://dav.example.com
# - 用户名：your_username
# - 密码：your_password
```

### 2. 创建同步任务

```bash
# 创建从 WebDAV 到本地的同步任务
cloud-disk-sync create-task \
  --name "下载备份" \
  --source my-webdav:/remote/backup \
  --target local:/home/user/backup \
  --schedule "0 2 * * *"  # 每天凌晨2点
```

### 3. 执行同步

```bash
# 立即执行同步
cloud-disk-sync run --task <task_id>

# 预览同步（不实际执行）
cloud-disk-sync run --task <task_id> --dry-run
```

## 代码示例

### 基本使用

```rust
use cloud_disk_sync::config::{AccountConfig, ProviderType, RetryPolicy};
use cloud_disk_sync::providers::{WebDavProvider, StorageProvider};
use std::collections::HashMap;
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 创建配置
    let mut credentials = HashMap::new();
    credentials.insert("url".to_string(), "https://dav.example.com".to_string());
    credentials.insert("username".to_string(), "your_username".to_string());
    credentials.insert("password".to_string(), "your_password".to_string());

    let config = AccountConfig {
        id: "webdav-001".to_string(),
        provider: ProviderType::WebDAV,
        name: "My WebDAV".to_string(),
        credentials,
        rate_limit: None,
        retry_policy: RetryPolicy::default(),
    };

    // 创建 provider
    let provider = WebDavProvider::new(&config).await?;

    // 上传文件
    let local_file = Path::new("./document.pdf");
    let result = provider.upload(local_file, "/documents/document.pdf").await?;
    println!("上传成功: {} bytes", result.bytes_uploaded);

    // 下载文件
    let download_path = Path::new("./downloaded.pdf");
    provider.download("/documents/document.pdf", download_path).await?;
    println!("下载成功");

    // 列出目录
    let files = provider.list("/documents").await?;
    for file in files {
        println!("{} - {} bytes", file.path, file.size);
    }

    Ok(())
}
```

### 批量操作

```rust
use cloud_disk_sync::providers::{StorageProvider, WebDavProvider};
use std::path::PathBuf;

async fn batch_upload(
    provider: &WebDavProvider,
    files: Vec<PathBuf>,
    remote_dir: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    for file in files {
        let file_name = file.file_name()
            .and_then(|n| n.to_str())
            .ok_or("Invalid filename")?;

        let remote_path = format!("{}/{}", remote_dir, file_name);

        match provider.upload(&file, &remote_path).await {
            Ok(result) => {
                println!("✓ {} - {} bytes", file_name, result.bytes_uploaded);
            }
            Err(e) => {
                eprintln!("✗ {} - 错误: {}", file_name, e);
            }
        }
    }

    Ok(())
}
```

### 目录同步

```rust
use cloud_disk_sync::providers::{WebDavProvider, StorageProvider};
use std::path::Path;
use walkdir::WalkDir;

async fn sync_directory(
    provider: &WebDavProvider,
    local_dir: &Path,
    remote_dir: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // 创建远程目录
    provider.mkdir(remote_dir).await?;

    // 遍历本地目录
    for entry in WalkDir::new(local_dir) {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            // 计算相对路径
            let relative = path.strip_prefix(local_dir)?;
            let remote_path = format!("{}/{}",
                                      remote_dir,
                                      relative.to_string_lossy()
            );

            // 上传文件
            provider.upload(path, &remote_path).await?;
            println!("已同步: {}", relative.display());
        }
    }

    Ok(())
}
```

### 错误处理

```rust
use cloud_disk_sync::providers::{WebDavProvider, StorageProvider};
use cloud_disk_sync::error::SyncError;

async fn safe_download(
    provider: &WebDavProvider,
    remote_path: &str,
    local_path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    match provider.download(remote_path, local_path).await {
        Ok(result) => {
            println!("下载成功: {} bytes", result.bytes_downloaded);
            Ok(())
        }
        Err(SyncError::Provider(provider_err)) => {
            eprintln!("Provider 错误: {}", provider_err);
            Err(provider_err.into())
        }
        Err(SyncError::Network(net_err)) => {
            eprintln!("网络错误: {}", net_err);
            // 可以重试
            Err(net_err.into())
        }
        Err(e) => {
            eprintln!("未知错误: {}", e);
            Err(e.into())
        }
    }
}
```

### 并发操作

```rust
use cloud_disk_sync::providers::{WebDavProvider, StorageProvider};
use tokio::task::JoinSet;
use std::sync::Arc;

async fn concurrent_upload(
    provider: Arc<WebDavProvider>,
    files: Vec<(std::path::PathBuf, String)>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut tasks = JoinSet::new();

    for (local_path, remote_path) in files {
        let provider = provider.clone();

        tasks.spawn(async move {
            provider.upload(&local_path, &remote_path).await
        });
    }

    let mut success = 0;
    let mut failed = 0;

    while let Some(result) = tasks.join_next().await {
        match result? {
            Ok(_) => success += 1,
            Err(_) => failed += 1,
        }
    }

    println!("上传完成: {} 成功, {} 失败", success, failed);
    Ok(())
}
```

## 支持的 WebDAV 服务器

已测试兼容的 WebDAV 服务器：

- ✅ Nginx + ngx_http_dav_module
- ✅ Apache + mod_dav
- ✅ Nextcloud
- ✅ ownCloud
- ✅ SabreDAV
- ✅ Windows IIS WebDAV

## 最佳实践

### 1. 使用限流

```rust
use cloud_disk_sync::config::RateLimitConfig;

let rate_limit = Some(RateLimitConfig {
requests_per_minute: 60,
max_concurrent: 5,
chunk_size: 1024 * 1024, // 1MB
});
```

### 2. 配置重试策略

```rust
use cloud_disk_sync::config::RetryPolicy;

let retry_policy = RetryPolicy {
max_retries: 3,
initial_delay_ms: 1000,
max_delay_ms: 10000,
backoff_factor: 2.0,
};
```

### 3. 大文件处理

对于大文件，建议：

- 使用分块上传
- 启用进度回调
- 实现断点续传

### 4. 安全建议

- 使用 HTTPS 连接
- 定期更新密码
- 避免在代码中硬编码凭证
- 使用环境变量或配置文件

## 故障排除

### 连接超时

增加超时时间：

```rust
let client = Client::builder()
.timeout(Duration::from_secs(60))
.build() ?;
```

### 认证失败

检查：

1. 用户名和密码是否正确
2. WebDAV 服务器是否启用
3. URL 是否正确（注意尾部斜杠）

### 上传失败

可能原因：

1. 磁盘空间不足
2. 文件权限问题
3. 文件大小超过限制

## 更多信息

- [API 文档](https://docs.rs/cloud_disk_sync)
- [问题反馈](https://github.com/your-repo/issues)
- [测试说明](./tests/README.md)
