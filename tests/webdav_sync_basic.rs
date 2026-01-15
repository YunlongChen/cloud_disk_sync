use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use tokio::sync::RwLock;
use tracing::info;
use warp::Filter;
use warp::http::Method;

use cloud_disk_sync::config::{AccountConfig, ConfigManager, DiffMode, ProviderType, RetryPolicy, SyncPolicy, SyncTask};
use cloud_disk_sync::providers::{StorageProvider, WebDavProvider};
use cloud_disk_sync::sync::diff::DiffAction;
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
async fn test_webdav_sync_directory_operations() {
    common::init_logging();

    // 启动 Mock WebDAV 服务器
    // 源：包含 new_dir/file.txt
    let (addr1, _store1) = start_mock_server_with_seed(vec![
        ("/new_dir/file.txt", "content", false),
        ("/new_dir", "", true),
    ]).await;

    // 目标：包含 old_dir/old.txt
    let (addr2, _store2) = start_mock_server_with_seed(vec![
        ("/old_dir/old.txt", "old content", false),
        ("/old_dir", "", true),
    ]).await;

    // 创建配置管理器和任务
    let mut config_manager = ConfigManager::new().unwrap();
    
    // 添加源账户
    let source_account = AccountConfig {
        id: "source_acc".to_string(),
        name: "Source".to_string(),
        provider: ProviderType::WebDAV,
        credentials: {
            let mut map = HashMap::new();
            map.insert("url".to_string(), format!("http://{}", addr1));
            map.insert("username".to_string(), "user".to_string());
            map.insert("password".to_string(), "pass".to_string());
            map
        },
        rate_limit: None,
        retry_policy: RetryPolicy::default(),
    };
    config_manager.add_account(source_account).unwrap();

    // 添加目标账户
    let target_account = AccountConfig {
        id: "target_acc".to_string(),
        name: "Target".to_string(),
        provider: ProviderType::WebDAV,
        credentials: {
            let mut map = HashMap::new();
            map.insert("url".to_string(), format!("http://{}", addr2));
            map.insert("username".to_string(), "user".to_string());
            map.insert("password".to_string(), "pass".to_string());
            map
        },
        rate_limit: None,
        retry_policy: RetryPolicy::default(),
    };
    config_manager.add_account(target_account).unwrap();

    // 添加同步任务
    let task = SyncTask {
        id: "test_dir_sync".to_string(),
        name: "Directory Sync Test".to_string(),
        source_account: "source_acc".to_string(),
        target_account: "target_acc".to_string(),
        source_path: "/".to_string(),
        target_path: "/".to_string(),
        sync_policy: Some(SyncPolicy {
            delete_orphans: true, // 开启删除孤儿文件/目录
            overwrite_existing: true,
            scan_cooldown_secs: 0,
        }),
        schedule: None,
        filters: vec![],
        encryption: None,
        diff_mode: DiffMode::Smart,
        preserve_metadata: false,
        verify_integrity: false,
    };
    config_manager.add_task(task.clone()).unwrap();

    // 创建 Provider (需要在 Engine 之外创建以通过 Box 传递)
    let source_provider = WebDavProvider::new(&config_manager.get_account("source_acc").unwrap()).await.unwrap();
    let target_provider = WebDavProvider::new(&config_manager.get_account("target_acc").unwrap()).await.unwrap();

    // 执行同步
    let mut engine = SyncEngine::new().await.unwrap();
    
    engine.register_provider("source_acc".to_string(), Box::new(source_provider));
    engine.register_provider("target_acc".to_string(), Box::new(target_provider));

    // 计算 Diff
    let diff = engine.calculate_diff_for_dry_run(&task).await.unwrap();
    
    // 验证 Diff 结果
    // 1. 应该包含 Upload new_dir (被标记为 Upload 因为源有目录，目标无)
    // 2. 应该包含 Upload new_dir/file.txt
    // 3. 应该包含 Delete old_dir
    // 4. 应该包含 Delete old_dir/old.txt
    
    // 注意：SyncEngine 的逻辑中，对于源是目录而目标没有的情况，使用了 Upload 动作而不是 CreateDir
    // 因为目录也被视为一种文件。所以我们这里检查 Upload 动作，且 is_dir 为 true
    let creates_dir = diff.files.iter().any(|f| 
        f.path == "new_dir/" && 
        f.action == DiffAction::Upload && 
        f.source_info.as_ref().map_or(false, |i| i.is_dir)
    );
    
    let deletes_dir = diff.files.iter().any(|f| f.path == "old_dir/" && f.action == DiffAction::Delete);
    
    assert!(creates_dir, "Should detect directory creation (as Upload). Diff: {:?}", diff.files);
    assert!(deletes_dir, "Should detect directory deletion. Diff: {:?}", diff.files);
    
    // 执行同步
    engine.sync(&task).await.unwrap();
    
    // 验证结果
    let target_provider = WebDavProvider::new(&config_manager.get_account("target_acc").unwrap()).await.unwrap();
    assert!(target_provider.exists("/new_dir").await.unwrap(), "new_dir should exist");
    assert!(!target_provider.exists("/old_dir").await.unwrap(), "old_dir should be deleted");
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
