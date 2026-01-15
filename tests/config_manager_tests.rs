use cloud_disk_sync::config::{
    AccountConfig, ConfigManager, DiffMode, ProviderType, RetryPolicy, SyncPolicy, SyncTask,
};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

fn get_test_config_path() -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push("disksync_test");
    if !path.exists() {
        fs::create_dir_all(&path).unwrap();
    }
    path.push(format!("config_{}.yaml", uuid::Uuid::new_v4()));
    path
}

/// 账户与任务管理 CRUD 测试：新增、保存、加载、更新、删除
#[test]
fn test_config_manager_crud() {
    let config_path = get_test_config_path();

    // 初始化配置管理器
    let mut mgr = ConfigManager::new_with_path(config_path.clone()).expect("创建配置管理器失败");

    // 新增账户
    let mut creds = HashMap::new();
    creds.insert("url".into(), "http://127.0.0.1:8080".into());
    creds.insert("username".into(), "u".into());
    creds.insert("password".into(), "p".into());
    let acc = AccountConfig {
        id: "acc1".into(),
        provider: ProviderType::WebDAV,
        name: "acc1-name".into(),
        credentials: creds.clone(),
        rate_limit: None,
        retry_policy: RetryPolicy::default(),
    };
    mgr.add_account(acc.clone()).unwrap();

    // 新增任务
    let task = SyncTask {
        id: "task1".into(),
        name: "task1-name".into(),
        source_account: "acc1".into(),
        source_path: "/file_root".into(),
        target_account: "acc1".into(),
        target_path: "/file_root_b".into(),
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
    mgr.add_task(task.clone()).unwrap();

    // 保存并重新加载
    mgr.save().unwrap();

    // 验证文件存在
    assert!(config_path.exists(), "配置文件未生成");

    let mut mgr2 = ConfigManager::new_with_path(config_path.clone()).expect("重新加载配置失败");
    assert!(mgr2.get_account("acc1").is_some(), "账户未保存");
    assert!(mgr2.get_task("task1").is_some(), "任务未保存");

    // 更新账户
    let mut acc_new = acc.clone();
    acc_new.name = "acc1-updated".into();
    mgr2.update_account(acc_new.clone()).unwrap();
    mgr2.save().unwrap();
    let mgr3 = ConfigManager::new_with_path(config_path.clone()).expect("重新加载配置失败2");
    assert_eq!(mgr3.get_account("acc1").unwrap().name, "acc1-updated");

    // 删除任务与账户
    let mut mgr4 = mgr3;
    mgr4.remove_task("task1").unwrap();
    mgr4.remove_account("acc1").unwrap();
    mgr4.save().unwrap();
    let mgr5 = ConfigManager::new_with_path(config_path.clone()).expect("重新加载配置失败3");
    assert!(mgr5.get_task("task1").is_none(), "任务未删除");
    assert!(mgr5.get_account("acc1").is_none(), "账户未删除");

    // 清理
    let _ = fs::remove_file(config_path);
}

#[test]
fn test_config_save_logging() {
    // 这是一个简单的测试，确保 save 不会 panic，并且确实写入了文件
    // 实际的日志输出通常需要 capturing subscriber 来验证，这里主要验证功能正确性
    let config_path = get_test_config_path();
    let mgr = ConfigManager::new_with_path(config_path.clone()).unwrap();
    mgr.save().unwrap();
    assert!(config_path.exists());
    let _ = fs::remove_file(config_path);
}
