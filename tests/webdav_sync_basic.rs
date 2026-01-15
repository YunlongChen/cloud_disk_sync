use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use tokio::sync::RwLock;
use tracing::info;
use warp::Filter;
use warp::http::Method;

use cloud_disk_sync::config::{AccountConfig, DiffMode, RetryPolicy, SyncPolicy, SyncTask};
use cloud_disk_sync::providers::{StorageProvider, WebDavProvider};
use cloud_disk_sync::sync::engine::SyncEngine;

mod common;
use common::{FileStore, InMemoryFile, start_mock_server_with_seed};

/// 基础WebDAV端到端同步测试：以 webdav1 为源，同步到 webdav2
#[tokio::test]
async fn test_webdav_sync_basic() {
    common::init_logging();
    let (addr1, _store1) = start_mock_server_with_seed(vec![
        ("/file_root/a.txt", "source a", false),
        ("/file_root/b.txt", "source b", false),
    ])
    .await;
    let (addr2, _store2) = start_mock_server_with_seed(vec![
        ("/file_root/b.txt", "target b (old)", false),
        ("/file_root/c.txt", "target c", false),
    ])
    .await;

    let webdav1 = AccountConfig {
        id: "webdav1".to_string(),
        provider: cloud_disk_sync::config::ProviderType::WebDAV,
        name: "webdav1".to_string(),
        credentials: {
            let mut c = HashMap::new();
            c.insert("url".to_string(), format!("http://{}", addr1));
            c.insert("username".to_string(), "user1".to_string());
            c.insert("password".to_string(), "pass1".to_string());
            c
        },
        rate_limit: None,
        retry_policy: RetryPolicy::default(),
    };

    let webdav2 = AccountConfig {
        id: "webdav2".to_string(),
        provider: cloud_disk_sync::config::ProviderType::WebDAV,
        name: "webdav2".to_string(),
        credentials: {
            let mut c = HashMap::new();
            c.insert("url".to_string(), format!("http://{}", addr2));
            c.insert("username".to_string(), "user2".to_string());
            c.insert("password".to_string(), "pass2".to_string());
            c
        },
        rate_limit: None,
        retry_policy: RetryPolicy::default(),
    };

    // 创建 Provider
    let src = WebDavProvider::new(&webdav1).await.unwrap();
    let dst = WebDavProvider::new(&webdav2).await.unwrap();

    // 等待容器启动（最大重试 10 次）
    let mut ready = false;
    for _ in 0..10 {
        if src.list("/file_root").await.is_ok() && dst.list("/file_root").await.is_ok() {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(ready, "webdav mock 服务未就绪");

    // 初始化同步引擎并注册 Provider
    let mut engine = SyncEngine::new().await.unwrap();
    engine.register_provider("webdav1".to_string(), Box::new(src));
    engine.register_provider("webdav2".to_string(), Box::new(dst));

    let task = SyncTask {
        id: "t_webdav_basic".to_string(),
        name: "basic webdav sync".to_string(),
        source_account: "webdav1".to_string(),
        source_path: "/file_root".to_string(),
        target_account: "webdav2".to_string(),
        target_path: "/file_root".to_string(),
        schedule: None,
        filters: vec![],
        encryption: None,
        diff_mode: DiffMode::Smart,
        preserve_metadata: false,
        verify_integrity: false,
        sync_policy: Some(SyncPolicy {
            delete_orphans: true,
            overwrite_existing: true,
            scan_cooldown_secs: 0,
        }),
    };

    let _report = engine.sync(&task).await.unwrap();

    let dst_provider = engine.get_provider("webdav2").unwrap();
    assert!(
        dst_provider.exists("/file_root/a.txt").await.unwrap(),
        "a.txt 未同步到目标"
    );
    assert!(
        !dst_provider.exists("/file_root/c.txt").await.unwrap(),
        "c.txt 未删除"
    );

    let temp_dir = std::env::temp_dir();
    let b_local = temp_dir.join("webdav_b_verify.txt");
    dst_provider
        .download("/file_root/b.txt", &b_local)
        .await
        .unwrap();
    let content = tokio::fs::read(&b_local).await.unwrap();
    assert_eq!(String::from_utf8_lossy(&content), "source b");
    tokio::fs::remove_file(&b_local).await.ok();
}

/// 大文件与多删除场景：源含 2MB 二进制文件，目标含多余文件，执行上传与批量删除
#[tokio::test]
async fn test_webdav_sync_large_and_multi_delete() {
    common::init_logging();
    // 启动源与目标 mock WebDAV
    let (addr1, _store1) = start_mock_server_with_seed(vec![
        ("/file_root/a.txt", "A NEW", false),
        ("/file_root/b.txt", "B NEWER", false),
    ])
    .await;
    // 为源添加大文件内容到临时文件后再通过 Provider 上传，以模拟真实上传路径
    let temp_dir = std::env::temp_dir();
    let large_local = temp_dir.join("mock_large_2mb.bin");
    let large_content = vec![7u8; 2 * 1024 * 1024];
    tokio::fs::write(&large_local, &large_content)
        .await
        .unwrap();

    let src_cfg = AccountConfig {
        id: "src_large".to_string(),
        provider: cloud_disk_sync::config::ProviderType::WebDAV,
        name: "src_large".to_string(),
        credentials: {
            let mut c = HashMap::new();
            c.insert("url".to_string(), format!("http://{}", addr1));
            c.insert("username".to_string(), "user1".to_string());
            c.insert("password".to_string(), "pass1".to_string());
            c
        },
        rate_limit: None,
        retry_policy: RetryPolicy::default(),
    };
    let src_provider = WebDavProvider::new(&src_cfg).await.unwrap();
    // 上传大文件到源
    src_provider
        .upload(&large_local, "/file_root/d.bin")
        .await
        .unwrap();

    let (addr2, _store2) = start_mock_server_with_seed(vec![
        ("/file_root/b.txt", "B OLD", false),
        ("/file_root/c.txt", "C REMOVE", false),
        ("/file_root/e.txt", "E REMOVE", false),
        ("/file_root/f.txt", "F REMOVE", false),
    ])
    .await;

    let dst_cfg = AccountConfig {
        id: "dst_large".to_string(),
        provider: cloud_disk_sync::config::ProviderType::WebDAV,
        name: "dst_large".to_string(),
        credentials: {
            let mut c = HashMap::new();
            c.insert("url".to_string(), format!("http://{}", addr2));
            c.insert("username".to_string(), "user2".to_string());
            c.insert("password".to_string(), "pass2".to_string());
            c
        },
        rate_limit: None,
        retry_policy: RetryPolicy::default(),
    };

    // 等待服务就绪
    let mut ready = false;
    for _ in 0..10 {
        if src_provider.list("/file_root").await.is_ok() {
            let dst_provider_try = WebDavProvider::new(&dst_cfg).await.unwrap();
            if dst_provider_try.list("/file_root").await.is_ok() {
                ready = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(ready, "webdav mock 服务未就绪(large)");

    // 注册到同步引擎
    let mut engine = SyncEngine::new().await.unwrap();
    engine.register_provider("webdav_src".to_string(), Box::new(src_provider));
    engine.register_provider(
        "webdav_dst".to_string(),
        Box::new(WebDavProvider::new(&dst_cfg).await.unwrap()),
    );

    let task = SyncTask {
        id: "t_webdav_large".to_string(),
        name: "large & multi-delete".to_string(),
        source_account: "webdav_src".to_string(),
        source_path: "/file_root".to_string(),
        target_account: "webdav_dst".to_string(),
        target_path: "/file_root".to_string(),
        schedule: None,
        filters: vec![],
        encryption: None,
        diff_mode: DiffMode::Smart,
        preserve_metadata: false,
        verify_integrity: false,
        sync_policy: Some(SyncPolicy {
            delete_orphans: true,
            overwrite_existing: true,
            scan_cooldown_secs: 0,
        }),
    };

    let report = engine.sync(&task).await.unwrap();
    assert!(
        report.errors.is_empty(),
        "同步报告包含错误: {:?}",
        report.errors
    );

    let dst_provider = engine.get_provider("webdav_dst").unwrap();
    // 验证新增/覆盖
    assert!(
        dst_provider.exists("/file_root/a.txt").await.unwrap(),
        "a.txt 未同步到目标"
    );
    // b.txt 内容应为源的 "B NEW"
    let b_local = temp_dir.join("webdav_b_large_verify.txt");
    dst_provider
        .download("/file_root/b.txt", &b_local)
        .await
        .unwrap();

    info!("测试日志输出！");

    let b_content = tokio::fs::read(&b_local).await.unwrap();
    assert_eq!(String::from_utf8_lossy(&b_content), "B NEWER");
    tokio::fs::remove_file(&b_local).await.ok();

    // 验证大文件存在且大小正确
    assert!(
        dst_provider.exists("/file_root/d.bin").await.unwrap(),
        "d.bin 未同步到目标"
    );
    let d_local = temp_dir.join("webdav_d_large_verify.bin");
    dst_provider
        .download("/file_root/d.bin", &d_local)
        .await
        .unwrap();
    let d_size = tokio::fs::metadata(&d_local).await.unwrap().len();
    assert_eq!(d_size, 2 * 1024 * 1024);
    tokio::fs::remove_file(&d_local).await.ok();

    // 验证多余文件被删除
    for p in ["/file_root/c.txt", "/file_root/e.txt", "/file_root/f.txt"] {
        assert!(!dst_provider.exists(p).await.unwrap(), "{} 未删除", p);
    }

    // 清理临时大文件
    tokio::fs::remove_file(&large_local).await.ok();
}

/// 策略：不删除目标孤立文件（delete_orphans=false）
#[tokio::test]
async fn test_webdav_sync_policy_no_delete_orphans() {
    common::init_logging();
    let (addr1, _s1) = start_mock_server_with_seed(vec![("/file_root/a.txt", "A", false)]).await;
    let (addr2, _s2) =
        start_mock_server_with_seed(vec![("/file_root/c.txt", "C OLD", false)]).await;

    let src_cfg = AccountConfig {
        id: "p_no_del_src".to_string(),
        provider: cloud_disk_sync::config::ProviderType::WebDAV,
        name: "p_no_del_src".to_string(),
        credentials: {
            let mut c = HashMap::new();
            c.insert("url".to_string(), format!("http://{}", addr1));
            c.insert("username".to_string(), "u".to_string());
            c.insert("password".to_string(), "p".to_string());
            c
        },
        rate_limit: None,
        retry_policy: RetryPolicy::default(),
    };
    let dst_cfg = AccountConfig {
        id: "p_no_del_dst".to_string(),
        provider: cloud_disk_sync::config::ProviderType::WebDAV,
        name: "p_no_del_dst".to_string(),
        credentials: {
            let mut c = HashMap::new();
            c.insert("url".to_string(), format!("http://{}", addr2));
            c.insert("username".to_string(), "u".to_string());
            c.insert("password".to_string(), "p".to_string());
            c
        },
        rate_limit: None,
        retry_policy: RetryPolicy::default(),
    };

    let mut engine = SyncEngine::new().await.unwrap();
    engine.register_provider(
        "src_nd".to_string(),
        Box::new(WebDavProvider::new(&src_cfg).await.unwrap()),
    );
    engine.register_provider(
        "dst_nd".to_string(),
        Box::new(WebDavProvider::new(&dst_cfg).await.unwrap()),
    );

    let task = SyncTask {
        id: "t_no_del".to_string(),
        name: "policy no delete".to_string(),
        source_account: "src_nd".to_string(),
        source_path: "/file_root".to_string(),
        target_account: "dst_nd".to_string(),
        target_path: "/file_root".to_string(),
        schedule: None,
        filters: vec![],
        encryption: None,
        diff_mode: DiffMode::Smart,
        preserve_metadata: false,
        verify_integrity: false,
        sync_policy: Some(SyncPolicy {
            delete_orphans: false,
            overwrite_existing: true,
            scan_cooldown_secs: 0,
        }),
    };

    engine.sync(&task).await.unwrap();
    let dst = engine.get_provider("dst_nd").unwrap();
    assert!(
        dst.exists("/file_root/a.txt").await.unwrap(),
        "a.txt 未被同步"
    );
    assert!(
        dst.exists("/file_root/c.txt").await.unwrap(),
        "c.txt 不应被删除"
    );
}

/// 策略：不覆盖目标已有文件（overwrite_existing=false）
#[tokio::test]
async fn test_webdav_sync_policy_no_overwrite() {
    common::init_logging();
    let (addr1, _s1) =
        start_mock_server_with_seed(vec![("/file_root/b.txt", "B NEW", false)]).await;
    let (addr2, _s2) =
        start_mock_server_with_seed(vec![("/file_root/b.txt", "B OLD", false)]).await;

    let src_cfg = AccountConfig {
        id: "p_no_ov_src".to_string(),
        provider: cloud_disk_sync::config::ProviderType::WebDAV,
        name: "p_no_ov_src".to_string(),
        credentials: {
            let mut c = HashMap::new();
            c.insert("url".to_string(), format!("http://{}", addr1));
            c.insert("username".to_string(), "u".to_string());
            c.insert("password".to_string(), "p".to_string());
            c
        },
        rate_limit: None,
        retry_policy: RetryPolicy::default(),
    };
    let dst_cfg = AccountConfig {
        id: "p_no_ov_dst".to_string(),
        provider: cloud_disk_sync::config::ProviderType::WebDAV,
        name: "p_no_ov_dst".to_string(),
        credentials: {
            let mut c = HashMap::new();
            c.insert("url".to_string(), format!("http://{}", addr2));
            c.insert("username".to_string(), "u".to_string());
            c.insert("password".to_string(), "p".to_string());
            c
        },
        rate_limit: None,
        retry_policy: RetryPolicy::default(),
    };

    let mut engine = SyncEngine::new().await.unwrap();
    engine.register_provider(
        "src_no".to_string(),
        Box::new(WebDavProvider::new(&src_cfg).await.unwrap()),
    );
    engine.register_provider(
        "dst_no".to_string(),
        Box::new(WebDavProvider::new(&dst_cfg).await.unwrap()),
    );

    let task = SyncTask {
        id: "t_no_ov".to_string(),
        name: "policy no overwrite".to_string(),
        source_account: "src_no".to_string(),
        source_path: "/file_root".to_string(),
        target_account: "dst_no".to_string(),
        target_path: "/file_root".to_string(),
        schedule: None,
        filters: vec![],
        encryption: None,
        diff_mode: DiffMode::Smart,
        preserve_metadata: false,
        verify_integrity: false,
        sync_policy: Some(SyncPolicy {
            delete_orphans: true,
            overwrite_existing: false,
            scan_cooldown_secs: 0,
        }),
    };

    engine.sync(&task).await.unwrap();
    let dst = engine.get_provider("dst_no").unwrap();
    let temp_dir = std::env::temp_dir();
    let verify = temp_dir.join("policy_no_overwrite_b.txt");
    dst.download("/file_root/b.txt", &verify).await.unwrap();
    let content = tokio::fs::read(&verify).await.unwrap();
    assert_eq!(String::from_utf8_lossy(&content), "B OLD");
    tokio::fs::remove_file(&verify).await.ok();
}

/// Diff 缓存与扫描限频：连续两次同步（冷却期内）不应检测到源端新增
#[tokio::test]
async fn test_webdav_diff_scan_cooldown() {
    common::init_logging();
    let (addr1, store1) =
        start_mock_server_with_seed(vec![("/file_root/a.txt", "A1", false)]).await;
    let (addr2, _s2) = start_mock_server_with_seed(vec![("/file_root/a.txt", "A0", false)]).await;

    let src_cfg = AccountConfig {
        id: "p_scan_src".to_string(),
        provider: cloud_disk_sync::config::ProviderType::WebDAV,
        name: "p_scan_src".to_string(),
        credentials: {
            let mut c = HashMap::new();
            c.insert("url".to_string(), format!("http://{}", addr1));
            c.insert("username".to_string(), "u".to_string());
            c.insert("password".to_string(), "p".to_string());
            c
        },
        rate_limit: None,
        retry_policy: RetryPolicy::default(),
    };
    let dst_cfg = AccountConfig {
        id: "p_scan_dst".to_string(),
        provider: cloud_disk_sync::config::ProviderType::WebDAV,
        name: "p_scan_dst".to_string(),
        credentials: {
            let mut c = HashMap::new();
            c.insert("url".to_string(), format!("http://{}", addr2));
            c.insert("username".to_string(), "u".to_string());
            c.insert("password".to_string(), "p".to_string());
            c
        },
        rate_limit: None,
        retry_policy: RetryPolicy::default(),
    };

    let mut engine = SyncEngine::new().await.unwrap();
    engine.register_provider(
        "src_sc".to_string(),
        Box::new(WebDavProvider::new(&src_cfg).await.unwrap()),
    );
    engine.register_provider(
        "dst_sc".to_string(),
        Box::new(WebDavProvider::new(&dst_cfg).await.unwrap()),
    );

    // 第一次同步：正常同步到 A1
    let task1 = SyncTask {
        id: "t_scan_1".to_string(),
        name: "scan 1".to_string(),
        source_account: "src_sc".to_string(),
        source_path: "/file_root".to_string(),
        target_account: "dst_sc".to_string(),
        target_path: "/file_root".to_string(),
        schedule: None,
        filters: vec![],
        encryption: None,
        diff_mode: DiffMode::Smart,
        preserve_metadata: false,
        verify_integrity: false,
        sync_policy: Some(SyncPolicy {
            delete_orphans: true,
            overwrite_existing: true,
            scan_cooldown_secs: 100,
        }),
    };
    engine.sync(&task1).await.unwrap();

    // 修改源端，新增 a2.txt
    {
        let mut files = store1.write().await;
        files.insert(
            "/file_root/a2.txt".to_string(),
            InMemoryFile {
                content: b"A2".to_vec(),
                is_dir: false,
            },
        );
    }

    // 第二次同步（冷却期内）：不应检测到 a2.txt
    let task2 = SyncTask {
        id: "t_scan_2".to_string(),
        name: "scan 2".to_string(),
        ..task1.clone()
    };
    engine.sync(&task2).await.unwrap();
    let dst = engine.get_provider("dst_sc").unwrap();
    assert!(
        !dst.exists("/file_root/a2.txt").await.unwrap(),
        "冷却期内不应同步 a2.txt"
    );

    // 第三次：新建引擎并关闭限频，a2.txt 应被同步
    let mut engine2 = SyncEngine::new().await.unwrap();
    engine2.register_provider(
        "src_sc".to_string(),
        Box::new(WebDavProvider::new(&src_cfg).await.unwrap()),
    );
    engine2.register_provider(
        "dst_sc".to_string(),
        Box::new(WebDavProvider::new(&dst_cfg).await.unwrap()),
    );
    let task3 = SyncTask {
        id: "t_scan_3".to_string(),
        name: "scan 3".to_string(),
        sync_policy: Some(SyncPolicy {
            delete_orphans: true,
            overwrite_existing: true,
            scan_cooldown_secs: 0,
        }),
        ..task1
    };
    engine2.sync(&task3).await.unwrap();
    let dst2 = engine2.get_provider("dst_sc").unwrap();
    assert!(
        dst2.exists("/file_root/a2.txt").await.unwrap(),
        "关闭限频后应同步 a2.txt"
    );
}
