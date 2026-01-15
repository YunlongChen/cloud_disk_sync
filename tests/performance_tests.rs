use cloud_disk_sync::config::{AccountConfig, DiffMode, RetryPolicy, SyncTask};
use cloud_disk_sync::providers::StorageProvider; // Added import
use cloud_disk_sync::providers::WebDavProvider;
use cloud_disk_sync::sync::engine::SyncEngine;
use std::collections::HashMap;
use std::time::Instant;

mod common;
use common::{generate_test_files, start_mock_server_with_seed};

#[tokio::test]
async fn test_sync_throughput_small_files() {
    common::init_logging();
    // 1. 准备数据
    let file_count = 100;
    let file_size = 1024; // 1KB

    // 启动空的 Mock Server
    let (addr1, _store1) = start_mock_server_with_seed(vec![]).await;
    let (addr2, _store2) = start_mock_server_with_seed(vec![]).await;

    // 2. 配置 WebDAV
    let src_cfg = AccountConfig {
        id: "src_perf".to_string(),
        provider: cloud_disk_sync::config::ProviderType::WebDAV,
        name: "src_perf".to_string(),
        credentials: {
            let mut c = HashMap::new();
            c.insert("url".to_string(), format!("http://{}", addr1));
            c.insert("username".to_string(), "u1".to_string());
            c.insert("password".to_string(), "p1".to_string());
            c
        },
        rate_limit: None,
        retry_policy: RetryPolicy::default(),
    };
    let dst_cfg = AccountConfig {
        id: "dst_perf".to_string(),
        provider: cloud_disk_sync::config::ProviderType::WebDAV,
        name: "dst_perf".to_string(),
        credentials: {
            let mut c = HashMap::new();
            c.insert("url".to_string(), format!("http://{}", addr2));
            c.insert("username".to_string(), "u2".to_string());
            c.insert("password".to_string(), "p2".to_string());
            c
        },
        rate_limit: None,
        retry_policy: RetryPolicy::default(),
    };

    let src_provider = WebDavProvider::new(&src_cfg).await.unwrap();
    let dst_provider = WebDavProvider::new(&dst_cfg).await.unwrap();

    // 3. 预热/初始化数据 (通过 Provider 上传)
    println!("Initializing {} files...", file_count);
    let temp_dir = std::env::temp_dir().join("perf_test_src");
    if temp_dir.exists() {
        tokio::fs::remove_dir_all(&temp_dir).await.ok();
    }
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();

    let files = generate_test_files(&temp_dir, file_count, file_size).await;

    for filename in &files {
        let local_path = temp_dir.join(filename);
        let remote_path = format!("/file_root/{}", filename);
        // Explicitly use StorageProvider trait method
        StorageProvider::upload(&src_provider, &local_path, &remote_path)
            .await
            .unwrap();
    }
    println!("Initialization complete.");

    // Debug: Check source files
    let list = src_provider.list("/file_root").await.unwrap();
    println!("Source files count on server: {}", list.len());
    if list.is_empty() {
        // Print store content if possible, or just fail early
        println!("Source provider list returned empty!");
    } else {
        println!("First file: {:?}", list[0]);
    }

    // 4. 运行同步性能测试
    let mut engine = SyncEngine::new().await.unwrap();
    engine.register_provider("src".to_string(), Box::new(src_provider));
    engine.register_provider("dst".to_string(), Box::new(dst_provider));

    let task = SyncTask {
        id: "t_perf".to_string(),
        name: "perf test".to_string(),
        source_account: "src".to_string(),
        source_path: "/file_root".to_string(),
        target_account: "dst".to_string(),
        target_path: "/file_root".to_string(),
        schedule: None,
        filters: vec![],
        encryption: None,
        diff_mode: DiffMode::Full,
        preserve_metadata: false,
        verify_integrity: false,
        sync_policy: None, // Added field
    };

    let start = Instant::now();
    let report = engine.sync(&task).await.unwrap();
    let duration = start.elapsed();

    println!("Sync completed in {:.2?}", duration);
    println!(
        "Throughput: {:.2} files/sec",
        file_count as f64 / duration.as_secs_f64()
    );
    println!("Report stats: {:?}", report.statistics);

    assert!(report.errors.is_empty(), "Errors: {:?}", report.errors);
    assert_eq!(report.statistics.files_synced, file_count); // Fixed field access

    // 清理
    tokio::fs::remove_dir_all(&temp_dir).await.ok();
}
