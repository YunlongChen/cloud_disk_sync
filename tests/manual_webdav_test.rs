use cloud_disk_sync::config::{AccountConfig, ProviderType, RetryPolicy};
use cloud_disk_sync::providers::{StorageProvider, WebDavProvider};
use std::collections::HashMap;

fn init_logging() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();
}

#[tokio::test]
#[ignore]
async fn test_manual_webdav_connection() {
    init_logging();
    // 1. 配置 WebDAV 账户
    let mut credentials = HashMap::new();
    credentials.insert("url".to_string(), "http://localhost:5242/dav".to_string());
    credentials.insert("username".to_string(), "webdav".to_string());
    credentials.insert("password".to_string(), "123456".to_string());

    let config = AccountConfig {
        id: "manual_test".to_string(),
        provider: ProviderType::WebDAV,
        name: "Manual WebDAV Test".to_string(),
        credentials,
        rate_limit: None,
        retry_policy: RetryPolicy::default(),
    };

    println!("正在连接 WebDAV 服务器: {}", config.credentials["url"]);

    // 2. 初始化 Provider
    let provider_result = WebDavProvider::new(&config).await;
    match provider_result {
        Ok(provider) => {
            println!("✅ Provider 初始化成功");

            // 3. 列出根目录
            println!("正在列出根目录 '/' 下的文件...");
            match provider.list("/").await {
                Ok(files) => {
                    println!("✅ 获取目录列表成功，共找到 {} 个文件/目录:", files.len());
                    println!(
                        "{:<10} {:<20} {:<10} {}",
                        "类型", "修改时间", "大小", "路径"
                    );
                    println!("{}", "-".repeat(80));
                    for file in files {
                        let type_str = if file.is_dir { "DIR" } else { "FILE" };
                        println!(
                            "{:<10} {:<20} {:<10} {}",
                            type_str, file.modified, file.size, file.path
                        );
                    }
                }
                Err(e) => {
                    eprintln!("❌ 获取目录列表失败: {:?}", e);
                    panic!("List failed");
                }
            }
        }
        Err(e) => {
            eprintln!("❌ Provider 初始化失败: {:?}", e);
            panic!("Init failed");
        }
    }
}
