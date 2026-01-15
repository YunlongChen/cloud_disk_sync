use cloud_disk_sync::config::{AccountConfig, DiffMode, RetryPolicy, SyncTask};
use cloud_disk_sync::providers::StorageProvider; // Added import
use cloud_disk_sync::providers::WebDavProvider;
use cloud_disk_sync::sync::engine::SyncEngine;
use std::collections::HashMap;
use std::time::Duration;

mod common;
use common::{FaultConfig, FaultInjectionProvider, start_mock_server_with_seed};

#[tokio::test]
async fn test_sync_with_latency() {
    common::init_logging();
    // 1. 启动 Mock Server
    let (addr1, _store1) =
        start_mock_server_with_seed(vec![("/file_root/a.txt", "content a", false)]).await;
    let (addr2, _store2) = start_mock_server_with_seed(vec![]).await;

    // 2. 配置 WebDAV 账户
    let src_cfg = AccountConfig {
        id: "src_latency".to_string(),
        provider: cloud_disk_sync::config::ProviderType::WebDAV,
        name: "src_latency".to_string(),
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
        id: "dst_latency".to_string(),
        provider: cloud_disk_sync::config::ProviderType::WebDAV,
        name: "dst_latency".to_string(),
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

    // 3. 创建 Provider 并注入延迟
    let src_provider = WebDavProvider::new(&src_cfg).await.unwrap();
    let dst_provider = WebDavProvider::new(&dst_cfg).await.unwrap();

    // 为源端注入 50ms 延迟
    let fault_src = FaultInjectionProvider::new(
        Box::new(src_provider),
        FaultConfig {
            latency_ms: 50,
            latency_jitter_ms: 10,
            error_rate: 0.0,
            ..Default::default()
        },
    );

    // 为目标端注入 50ms 延迟
    let fault_dst = FaultInjectionProvider::new(
        Box::new(dst_provider),
        FaultConfig {
            latency_ms: 50,
            latency_jitter_ms: 10,
            error_rate: 0.0,
            ..Default::default()
        },
    );

    // 4. 等待 Mock Server 就绪 (简单重试)
    let mut ready = false;
    for _ in 0..10 {
        if fault_src.list("/file_root").await.is_ok() && fault_dst.list("/file_root").await.is_ok()
        {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(ready, "Mock server not ready");

    // 5. 运行同步
    let mut engine = SyncEngine::new().await.unwrap();
    engine.register_provider("src".to_string(), Box::new(fault_src));
    engine.register_provider("dst".to_string(), Box::new(fault_dst));

    let task = SyncTask {
        id: "t_latency".to_string(),
        name: "latency test".to_string(),
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

    let report = engine.sync(&task).await.unwrap();
    assert!(
        report.errors.is_empty(),
        "Sync failed with errors: {:?}",
        report.errors
    );
    assert_eq!(report.statistics.files_synced, 1); // Fixed field access

    // 验证目标端文件
    let dst_check = engine.get_provider("dst").unwrap();
    assert!(dst_check.exists("/file_root/a.txt").await.unwrap());
}

#[tokio::test]
async fn test_sync_with_random_errors() {
    common::init_logging();
    // 1. 启动 Mock Server
    let (addr1, _store1) =
        start_mock_server_with_seed(vec![("/file_root/b.txt", "content b", false)]).await;
    let (addr2, _store2) = start_mock_server_with_seed(vec![]).await;

    // 2. 配置
    let src_cfg = AccountConfig {
        id: "src_err".to_string(),
        provider: cloud_disk_sync::config::ProviderType::WebDAV,
        name: "src_err".to_string(),
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
        id: "dst_err".to_string(),
        provider: cloud_disk_sync::config::ProviderType::WebDAV,
        name: "dst_err".to_string(),
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

    // 3. 注入高错误率 (20%)
    let src_provider = WebDavProvider::new(&src_cfg).await.unwrap();
    let fault_src = FaultInjectionProvider::new(
        Box::new(src_provider),
        FaultConfig {
            error_rate: 0.2,
            error_type: "server".to_string(),
            ..Default::default()
        },
    );

    let dst_provider = WebDavProvider::new(&dst_cfg).await.unwrap();

    // 4. 运行同步
    let mut engine = SyncEngine::new().await.unwrap();
    engine.register_provider("src".to_string(), Box::new(fault_src));
    engine.register_provider("dst".to_string(), Box::new(dst_provider));

    let task = SyncTask {
        id: "t_error".to_string(),
        name: "error test".to_string(),
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

    // 此时可能因为没有自动重试而失败，或者报告中有错误
    let report = engine.sync(&task).await.unwrap();

    println!(
        "Sync report with faults: success={}, errors={}",
        report.statistics.files_synced, // Fixed field access
        report.errors.len()
    );
}
