use cloud_disk_sync::config::{AccountConfig, DiffMode, RetryPolicy, SyncPolicy, SyncTask};
use cloud_disk_sync::providers::StorageProvider;
use cloud_disk_sync::providers::WebDavProvider;
use cloud_disk_sync::sync::engine::SyncEngine;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

mod common;
use common::{generate_deep_structure, start_mock_server_with_seed};

#[tokio::test]
async fn test_consistency_deep_nesting() {
    common::init_logging();
    // 1. 启动 Mock Server
    let (addr1, _store1) = start_mock_server_with_seed(vec![]).await;
    let (addr2, _store2) = start_mock_server_with_seed(vec![]).await;

    // 2. 配置账户
    let src_cfg = create_test_config("src_deep", addr1);
    let dst_cfg = create_test_config("dst_deep", addr2);

    let src_provider = WebDavProvider::new(&src_cfg).await.unwrap();
    let dst_provider = WebDavProvider::new(&dst_cfg).await.unwrap();

    // 3. 生成深层目录结构 (5层，每层5个文件)
    // 使用临时目录生成，然后上传到 source mock server
    let temp_dir = std::env::temp_dir().join("deep_nest_src");
    if temp_dir.exists() {
        tokio::fs::remove_dir_all(&temp_dir).await.ok();
    }
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();

    // generate_deep_structure is in common, let's use it
    // Wait, common::generate_deep_structure generates files locally.
    // I need to upload them to src_provider.
    common::generate_deep_structure(&temp_dir, 5, 5).await;

    // 递归上传
    upload_recursive(&src_provider, &temp_dir, "/file_root").await;

    // 4. 执行同步
    let mut engine = SyncEngine::new().await.unwrap();
    engine.register_provider("src".to_string(), Box::new(src_provider));
    engine.register_provider("dst".to_string(), Box::new(dst_provider));

    let task = SyncTask {
        id: "t_deep".to_string(),
        name: "deep sync".to_string(),
        source_account: "src".to_string(),
        source_path: "/file_root".to_string(),
        target_account: "dst".to_string(),
        target_path: "/file_root".to_string(),
        schedule: None,
        filters: vec![],
        encryption: None,
        diff_mode: DiffMode::Full,
        preserve_metadata: false,
        verify_integrity: true, // 开启校验
        sync_policy: None,
    };

    let report = engine.sync(&task).await.unwrap();

    // 5. 验证
    assert!(report.errors.is_empty(), "Errors: {:?}", report.errors);
    // 5 levels * 5 files + maybe root files?
    // generate_deep_structure:
    // level_0/ (5 files)
    // level_0/level_1/ (5 files)
    // ...
    // Total files = 5 * 5 = 25 files.
    assert_eq!(report.statistics.files_synced, 25);

    // 验证目标端文件是否存在
    let dst_check = engine.get_provider("dst").unwrap();
    assert!(
        dst_check
            .exists("/file_root/level_0/test_file_0.dat")
            .await
            .unwrap()
    );
    assert!(
        dst_check
            .exists("/file_root/level_0/level_1/level_2/level_3/level_4/test_file_4.dat")
            .await
            .unwrap()
    );

    // 清理
    tokio::fs::remove_dir_all(&temp_dir).await.ok();
}

#[tokio::test]
async fn test_consistency_conflict_skip() {
    common::init_logging();
    // 测试策略：overwrite_existing = false (跳过已存在)

    // 1. 启动 Mock Server
    let (addr1, _store1) = start_mock_server_with_seed(vec![
        ("/file_root/conflict.txt", "source content", false),
        ("/file_root/new.txt", "new content", false),
    ])
    .await;

    let (addr2, _store2) =
        start_mock_server_with_seed(vec![("/file_root/conflict.txt", "target content", false)])
            .await;

    // 2. 配置
    let src_cfg = create_test_config("src_conflict", addr1);
    let dst_cfg = create_test_config("dst_conflict", addr2);

    let src_provider = WebDavProvider::new(&src_cfg).await.unwrap();
    let dst_provider = WebDavProvider::new(&dst_cfg).await.unwrap();

    // 3. 同步
    let mut engine = SyncEngine::new().await.unwrap();
    engine.register_provider("src".to_string(), Box::new(src_provider));
    engine.register_provider("dst".to_string(), Box::new(dst_provider));

    let task = SyncTask {
        id: "t_conflict".to_string(),
        name: "conflict test".to_string(),
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
        sync_policy: Some(SyncPolicy {
            delete_orphans: false,
            overwrite_existing: false, // 关键：不覆盖
            scan_cooldown_secs: 0,
        }),
    };

    let report = engine.sync(&task).await.unwrap();

    // 4. 验证
    // Should upload "new.txt" (1 success)
    // Should skip "conflict.txt" (1 skipped? Or just not in diff?)
    // In engine implementation:
    // if !overwrite_existing && target_exists { continue; } -> It is not added to diff.
    // So files_synced = 1.

    assert_eq!(report.statistics.files_synced, 1);

    // Verify content of conflict.txt on target is UNCHANGED
    let dst_check = engine.get_provider("dst").unwrap();
    let temp_dl = std::env::temp_dir().join("conflict_check.txt");
    dst_check
        .download("/file_root/conflict.txt", &temp_dl)
        .await
        .unwrap();
    let content = tokio::fs::read_to_string(&temp_dl).await.unwrap();
    assert_eq!(content, "target content"); // Should remain target content
}

#[tokio::test]
async fn test_consistency_conflict_overwrite() {
    common::init_logging();
    // 测试策略：overwrite_existing = true (覆盖)

    let (addr1, _store1) =
        start_mock_server_with_seed(vec![("/file_root/conflict.txt", "source content", false)])
            .await;

    let (addr2, _store2) =
        start_mock_server_with_seed(vec![("/file_root/conflict.txt", "target content", false)])
            .await;

    let src_cfg = create_test_config("src_over", addr1);
    let dst_cfg = create_test_config("dst_over", addr2);

    let src_provider = WebDavProvider::new(&src_cfg).await.unwrap();
    let dst_provider = WebDavProvider::new(&dst_cfg).await.unwrap();

    let mut engine = SyncEngine::new().await.unwrap();
    engine.register_provider("src".to_string(), Box::new(src_provider));
    engine.register_provider("dst".to_string(), Box::new(dst_provider));

    let task = SyncTask {
        id: "t_over".to_string(),
        name: "overwrite test".to_string(),
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
        sync_policy: Some(SyncPolicy {
            delete_orphans: false,
            overwrite_existing: true, // 关键：覆盖
            scan_cooldown_secs: 0,
        }),
    };

    let report = engine.sync(&task).await.unwrap();

    // 4. 验证
    // Should upload "conflict.txt"
    assert_eq!(report.statistics.files_synced, 1);

    // Verify content changed
    let dst_check = engine.get_provider("dst").unwrap();
    let temp_dl = std::env::temp_dir().join("overwrite_check.txt");
    dst_check
        .download("/file_root/conflict.txt", &temp_dl)
        .await
        .unwrap();
    let content = tokio::fs::read_to_string(&temp_dl).await.unwrap();
    assert_eq!(content, "source content");
}

// Helpers
fn create_test_config(id: &str, addr: SocketAddr) -> AccountConfig {
    AccountConfig {
        id: id.to_string(),
        provider: cloud_disk_sync::config::ProviderType::WebDAV,
        name: id.to_string(),
        credentials: {
            let mut c = HashMap::new();
            c.insert("url".to_string(), format!("http://{}", addr));
            c.insert("username".to_string(), "u".to_string());
            c.insert("password".to_string(), "p".to_string());
            c
        },
        rate_limit: None,
        retry_policy: RetryPolicy::default(),
    }
}

async fn upload_recursive(
    provider: &WebDavProvider,
    local_dir: &std::path::Path,
    remote_base: &str,
) {
    let mut stack = vec![local_dir.to_path_buf()];
    let base_len = local_dir.parent().unwrap().to_str().unwrap().len(); // Parent len to get relative path

    // We need relative path from local_dir
    // local_dir: /tmp/deep
    // file: /tmp/deep/level_0/file.txt
    // rel: level_0/file.txt

    while let Some(dir) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&dir).await.unwrap();
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            let rel_path = path.strip_prefix(local_dir).unwrap();
            let remote_path = format!(
                "{}/{}",
                remote_base.trim_end_matches('/'),
                rel_path.to_string_lossy().replace("\\", "/")
            );

            if path.is_dir() {
                provider.mkdir(&remote_path).await.ok(); // ignore if exists
                stack.push(path);
            } else {
                // Ensure parent exists? mkdir should handle?
                // WebDavProvider::mkdir only creates one level?
                // Usually WebDAV requires parents. My mkdir mock implementation might not enforce strictness or my recursive approach handles it if I traverse top-down.
                // stack pop order is LIFO (DFS).
                // read_dir order is undefined.
                // Better to ensure parent exists.

                // For simplicity, just try upload. WebDavProvider::upload creates parent dirs locally for download, but for upload?
                // WebDavProvider::upload just PUTs. The server might require parent.
                // MockServer automatically creates parents?
                // tests/common/mod.rs MockServer put_route:
                // files.insert(path_str, ...)
                // It inserts into HashMap. It doesn't check parents. So it works.

                StorageProvider::upload(provider, &path, &remote_path)
                    .await
                    .unwrap();
            }
        }
    }
}
