use async_trait::async_trait;
use cloud_disk_sync::config::{DiffMode, SyncPolicy, SyncTask};
use cloud_disk_sync::error::SyncError;
use cloud_disk_sync::providers::{DownloadResult, FileInfo, StorageProvider, UploadResult};
use cloud_disk_sync::sync::diff::DiffAction;
use cloud_disk_sync::sync::engine::SyncEngine;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

// Mock Provider for deterministic testing
#[derive(Clone)]
struct MockProvider {
    files: Arc<Mutex<HashMap<String, FileInfo>>>,
}

impl MockProvider {
    fn new(files: Vec<FileInfo>) -> Self {
        let mut map = HashMap::new();
        for f in files {
            map.insert(f.path.clone(), f);
        }
        Self {
            files: Arc::new(Mutex::new(map)),
        }
    }
}

#[async_trait]
impl StorageProvider for MockProvider {
    async fn list(&self, _path: &str) -> Result<Vec<FileInfo>, SyncError> {
        let files = self.files.lock().unwrap();
        Ok(files.values().cloned().collect())
    }

    async fn upload(
        &self,
        _local_path: &Path,
        _remote_path: &str,
    ) -> Result<UploadResult, SyncError> {
        Ok(UploadResult {
            bytes_uploaded: 0,
            file_size: 0,
            checksum: None,
            elapsed_time: std::time::Duration::from_secs(0),
        })
    }

    async fn download(
        &self,
        _remote_path: &str,
        _local_path: &Path,
    ) -> Result<DownloadResult, SyncError> {
        Ok(DownloadResult {
            bytes_downloaded: 0,
            file_size: 0,
            checksum: None,
            elapsed_time: std::time::Duration::from_secs(0),
        })
    }

    async fn delete(&self, _path: &str) -> Result<(), SyncError> {
        Ok(())
    }

    async fn mkdir(&self, _path: &str) -> Result<(), SyncError> {
        Ok(())
    }

    async fn stat(&self, path: &str) -> Result<FileInfo, SyncError> {
        let files = self.files.lock().unwrap();
        files.get(path).cloned().ok_or(SyncError::Provider(
            cloud_disk_sync::error::ProviderError::NotFound(path.to_string()),
        ))
    }

    async fn exists(&self, path: &str) -> Result<bool, SyncError> {
        let files = self.files.lock().unwrap();
        Ok(files.contains_key(path))
    }
}

fn create_file_info(path: &str, size: u64, modified: i64) -> FileInfo {
    FileInfo {
        path: path.to_string(),
        size,
        modified,
        is_dir: false,
        hash: None,
    }
}

async fn setup_engine(
    src_files: Vec<FileInfo>,
    dst_files: Vec<FileInfo>,
) -> (SyncEngine, SyncTask) {
    let mut engine = SyncEngine::new().await.unwrap();

    let src_provider = MockProvider::new(src_files);
    let dst_provider = MockProvider::new(dst_files);

    engine.register_provider("src".to_string(), Box::new(src_provider));
    engine.register_provider("dst".to_string(), Box::new(dst_provider));

    let task = SyncTask {
        id: "test_task".to_string(),
        name: "Test Task".to_string(),
        source_account: "src".to_string(),
        source_path: "/".to_string(),
        target_account: "dst".to_string(),
        target_path: "/".to_string(),
        schedule: None,
        filters: vec![],
        encryption: None,
        diff_mode: DiffMode::Full,
        preserve_metadata: true,
        verify_integrity: false,
        sync_policy: Some(SyncPolicy {
            delete_orphans: true,
            overwrite_existing: true,
            scan_cooldown_secs: 0,
        }),
    };

    (engine, task)
}

#[tokio::test]
async fn test_diff_new_file() {
    let src_files = vec![create_file_info("/a.txt", 100, 1000)];
    let dst_files = vec![];

    let (engine, task) = setup_engine(src_files, dst_files).await;
    let diff = engine.calculate_diff_for_dry_run(&task).await.unwrap();

    assert_eq!(diff.files.len(), 1);
    let file = &diff.files[0];
    assert_eq!(file.path, "a.txt"); // Normalized relative path
    assert!(matches!(file.action, DiffAction::Upload));
}

#[tokio::test]
async fn test_diff_delete_file() {
    let src_files = vec![];
    let dst_files = vec![create_file_info("/b.txt", 100, 1000)];

    let (engine, task) = setup_engine(src_files, dst_files).await;
    let diff = engine.calculate_diff_for_dry_run(&task).await.unwrap();

    assert_eq!(diff.files.len(), 1);
    let file = &diff.files[0];
    assert_eq!(file.path, "b.txt");
    assert!(matches!(file.action, DiffAction::Delete));
}

#[tokio::test]
async fn test_diff_delete_file_disabled() {
    let src_files = vec![];
    let dst_files = vec![create_file_info("/b.txt", 100, 1000)];

    let (engine, mut task) = setup_engine(src_files, dst_files).await;
    // Disable delete orphans
    if let Some(policy) = &mut task.sync_policy {
        policy.delete_orphans = false;
    }

    let diff = engine.calculate_diff_for_dry_run(&task).await.unwrap();

    assert_eq!(diff.files.len(), 1);
    let file = &diff.files[0];
    assert_eq!(file.path, "b.txt");
    // Should be Unchanged with target_only tag
    assert!(matches!(file.action, DiffAction::Unchanged));
    assert!(file.tags.contains(&"target_only".to_string()));
}

#[tokio::test]
async fn test_diff_update_size() {
    let src_files = vec![create_file_info("/c.txt", 200, 1000)];
    let dst_files = vec![create_file_info("/c.txt", 100, 1000)];

    let (engine, task) = setup_engine(src_files, dst_files).await;
    let diff = engine.calculate_diff_for_dry_run(&task).await.unwrap();

    assert_eq!(diff.files.len(), 1);
    let file = &diff.files[0];
    assert_eq!(file.path, "c.txt");
    assert!(matches!(file.action, DiffAction::Update));
}

#[tokio::test]
async fn test_diff_update_time() {
    let src_files = vec![create_file_info("/d.txt", 100, 2000)];
    let dst_files = vec![create_file_info("/d.txt", 100, 1000)];

    let (engine, task) = setup_engine(src_files, dst_files).await;
    let diff = engine.calculate_diff_for_dry_run(&task).await.unwrap();

    assert_eq!(diff.files.len(), 1);
    let file = &diff.files[0];
    assert_eq!(file.path, "d.txt");
    assert!(matches!(file.action, DiffAction::Update));
}

#[tokio::test]
async fn test_diff_unchanged() {
    let src_files = vec![create_file_info("/e.txt", 100, 1000)];
    let dst_files = vec![create_file_info("/e.txt", 100, 1000)];

    let (engine, task) = setup_engine(src_files, dst_files).await;
    let diff = engine.calculate_diff_for_dry_run(&task).await.unwrap();

    // Even unchanged files are returned in the detailed diff result (as per user requirement "show all file info")
    // Wait, let's check calculate_diff implementation.
    // Yes, it adds FileDiff::unchanged

    assert_eq!(diff.files.len(), 1);
    let file = &diff.files[0];
    assert_eq!(file.path, "e.txt");
    assert!(matches!(file.action, DiffAction::Unchanged));
}

#[tokio::test]
async fn test_diff_skip_overwrite() {
    let src_files = vec![create_file_info("/f.txt", 200, 1000)];
    let dst_files = vec![create_file_info("/f.txt", 100, 1000)];

    let (engine, mut task) = setup_engine(src_files, dst_files).await;
    // Disable overwrite
    if let Some(policy) = &mut task.sync_policy {
        policy.overwrite_existing = false;
    }

    let diff = engine.calculate_diff_for_dry_run(&task).await.unwrap();

    assert_eq!(diff.files.len(), 1);
    let file = &diff.files[0];
    assert_eq!(file.path, "f.txt");
    // Should be Unchanged with skipped_overwrite tag
    assert!(matches!(file.action, DiffAction::Unchanged));
    assert!(file.tags.contains(&"skipped_overwrite".to_string()));
}
