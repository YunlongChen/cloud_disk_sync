use crate::config::{DiffMode, SyncTask};
use crate::encryption::EncryptionManager;
use crate::error::{ProviderError, SyncError};
use crate::providers::StorageProvider;
use crate::report::SyncReport;
use crate::sync::diff::{ChecksumType, DiffAction, DiffResult, FileDiff};
use dashmap::DashMap;
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

pub struct SyncEngine {
    providers: HashMap<String, Box<dyn StorageProvider>>,
    encryption_manager: EncryptionManager,
    diff_cache: DashMap<String, FileDiff>,
    resume_store: Arc<Mutex<Connection>>,
}

impl SyncEngine {
    pub async fn new() -> Result<Self, SyncError> {
        let db_path = dirs::data_dir()
            .ok_or(SyncError::Unknown(String::from("Failed to obtain data_dir")))?
            .join("disksync");

        std::fs::create_dir_all(&db_path)?;

        let db_path = db_path.join("resume.db");
        let conn = Connection::open(&db_path)?;

        // 创建简历表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS resume_data (
                task_id TEXT NOT NULL,
                file_path TEXT NOT NULL,
                last_modified INTEGER NOT NULL,
                file_size INTEGER NOT NULL,
                checksum TEXT,
                status TEXT NOT NULL,
                PRIMARY KEY (task_id, file_path)
            )",
            [],
        )?;

        Ok(Self {
            providers: HashMap::new(),
            encryption_manager: EncryptionManager::new(),
            diff_cache: DashMap::new(),
            resume_store: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn get_provider(&self, account_id: &str) -> Option<&Box<dyn StorageProvider>> {
        self.providers.get(account_id)
    }

    pub async fn walk_directory(
        &self,
        provider: &dyn StorageProvider,
        root: &str,
    ) -> Result<std::collections::HashMap<String, crate::sync::diff::FileMetadata>, SyncError> {
        let mut map = std::collections::HashMap::new();
        let entries = provider.list(root).await?;
        for e in entries {
            let path = std::path::PathBuf::from(e.path.clone());
            let meta = crate::sync::diff::FileMetadata::from_path(&path)?;
            map.insert(e.path, meta);
        }
        Ok(map)
    }

    fn create_temp_file(&self) -> Result<std::path::PathBuf, SyncError> {
        let name = format!("sync_{}.tmp", uuid::Uuid::new_v4());
        let path = std::env::temp_dir().join(name);
        std::fs::File::create(&path)?;
        Ok(path)
    }

    fn cleanup_temp_file(&self, path: &std::path::Path) -> Result<(), SyncError> {
        std::fs::remove_file(path)?;
        Ok(())
    }

    pub async fn sync(&mut self, task: &SyncTask) -> Result<SyncReport, SyncError> {
        let mut report = SyncReport::new();
        report.task_id = task.id.clone();

        // 计算文件差异（限定借用作用域）
        let diff = {
            let source_provider = self.get_provider(&task.source_account)
                .ok_or(SyncError::Provider(ProviderError::NotFound(task.source_account.clone())))?;
            let target_provider = self.get_provider(&task.target_account)
                .ok_or(SyncError::Provider(ProviderError::NotFound(task.target_account.clone())))?;
            self.calculate_diff(
                source_provider.as_ref(),
                target_provider.as_ref(),
                &task.source_path,
                &task.target_path,
                &task.diff_mode,
            ).await?
        };

        // 执行同步
        for file_diff in diff.files {
            match file_diff.action {
                DiffAction::Upload => {
                    // 重新获取 provider，避免与 &self 的可变借用冲突
                    let source_provider = self.get_provider(&task.source_account)
                        .ok_or(SyncError::Provider(ProviderError::NotFound(task.source_account.clone())))?;
                    let target_provider = self.get_provider(&task.target_account)
                        .ok_or(SyncError::Provider(ProviderError::NotFound(task.target_account.clone())))?;
                    self.sync_file(
                        source_provider.as_ref(),
                        target_provider.as_ref(),
                        &file_diff,
                        task,
                        &mut report,
                    ).await?;
                }
                DiffAction::Download => {
                    // 反向同步
                }
                DiffAction::Delete => {
                    // 删除目标文件
                }
                DiffAction::Conflict => {
                    report.add_conflict(&file_diff.path);
                }
                _ => {}
            }
        }
        Ok(report)
    }
}

// 进度结构体与结果类型
pub struct VerificationProgress {
    pub current_path: String,
    pub current_file: usize,
    pub total_files: usize,
}

pub struct SyncProgress {
    pub current_file: String,
    pub current_file_size: u64,
    pub transferred: u64,
    pub total: u64,
    pub percentage: f64,
    pub speed: f64,
}

pub struct VerificationResult {
    pub total_files: usize,
    pub checked_files: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

impl VerificationResult {
    pub fn new() -> Self {
        VerificationResult {
            total_files: 0,
            checked_files: 0,
            passed: 0,
            failed: 0,
            skipped: 0,
            errors: vec![],
        }
    }
}

pub struct RepairResult {
    pub repaired_files: usize,
    pub repaired_bytes: u64,
}

impl Default for RepairResult {
    fn default() -> Self {
        RepairResult {
            repaired_files: 0,
            repaired_bytes: 0,
        }
    }
}

pub struct DryRunResult {
    pub total_files: usize,
    pub files_to_upload: usize,
    pub files_to_download: usize,
    pub files_to_delete: usize,
    pub total_size: u64,
    pub conflicts: Vec<String>,
    pub files: Vec<FileDiff>,
}

impl Default for DryRunResult {
    fn default() -> Self {
        DryRunResult {
            total_files: 0,
            files_to_upload: 0,
            files_to_download: 0,
            files_to_delete: 0,
            total_size: 0,
            conflicts: vec![],
            files: vec![],
        }
    }
}

impl SyncEngine {
    pub async fn verify_integrity(
        &self,
        task: &SyncTask,
        _verify_all: bool,
        progress_callback: impl Fn(VerificationProgress),
    ) -> Result<VerificationResult, SyncError> {
        let source_provider = self.get_provider(&task.source_account)
            .ok_or(SyncError::Provider(ProviderError::NotFound(task.source_account.clone())))?;
        let target_provider = self.get_provider(&task.target_account)
            .ok_or(SyncError::Provider(ProviderError::NotFound(task.target_account.clone())))?;

        let mut result = VerificationResult::new();
        let source_files = self.walk_directory(source_provider.as_ref(), &task.source_path).await?;
        let target_files = self.walk_directory(target_provider.as_ref(), &task.target_path).await?;

        for (path, source_info) in &source_files {
            progress_callback(VerificationProgress {
                current_path: path.clone(),
                current_file: result.checked_files + 1,
                total_files: source_files.len(),
            });
            if let Some(target_info) = target_files.get(path) {
                if let (Some(source_hash), Some(target_hash)) = (&source_info.file_hash, &target_info.file_hash) {
                    if source_hash == target_hash {
                        result.passed += 1;
                    } else {
                        result.failed += 1;
                        result.errors.push(format!("文件哈希不匹配: {}", path));
                    }
                } else if source_info.size == target_info.size {
                    result.passed += 1;
                } else {
                    result.failed += 1;
                    result.errors.push(format!("文件大小不匹配: {}", path));
                }
            } else {
                result.skipped += 1;
            }
            result.checked_files += 1;
        }
        result.total_files = source_files.len();
        Ok(result)
    }

    pub async fn repair_integrity(
        &self,
        _task: &SyncTask,
        _verification_result: &VerificationResult,
    ) -> Result<RepairResult, SyncError> {
        Ok(RepairResult::default())
    }

    pub async fn sync_with_progress(
        &self,
        _task: &SyncTask,
        _progress_callback: impl Fn(SyncProgress),
    ) -> Result<SyncReport, SyncError> {
        Ok(SyncReport::new())
    }

    pub async fn calculate_diff_for_dry_run(
        &self,
        _task: &SyncTask,
    ) -> Result<DryRunResult, SyncError> {
        Ok(DryRunResult::default())
    }
}

impl SyncEngine {
    async fn calculate_diff(
        &self,
        source: &dyn StorageProvider,
        target: &dyn StorageProvider,
        source_path: &str,
        target_path: &str,
        diff_mode: &DiffMode,
    ) -> Result<DiffResult, SyncError> {
        let source_files = self.walk_directory(source, source_path).await?;
        let target_files = self.walk_directory(target, target_path).await?;

        let mut diff = DiffResult::new();

        // 使用哈希比较差异
        for (path, source_info) in &source_files {
            let target_info = target_files.get(path);

            match target_info {
                Some(target_info) => {
                    if source_info.file_hash != target_info.file_hash {
                        diff.add_file(FileDiff {
                            path: path.clone(),
                            action: DiffAction::Upload,
                            source_info: None,
                            target_info: None,
                            change_details: Default::default(),
                            size_diff: 0,
                            is_large_file: false,
                            requires_chunking: false,
                            requires_encryption: false,
                            priority: 0,
                            estimated_duration_ms: 0,
                            last_processed: None,
                            retry_count: 0,
                            error_message: None,
                            tags: vec![],
                            checksum_type: ChecksumType::Md5,
                            source_checksum: None,
                            target_checksum: None,
                            diff_id: "".to_string(),
                            created_at: SystemTime::now(),
                        });
                    }
                }
                None => {
                    diff.add_file(FileDiff {
                        path: path.clone(),
                        action: DiffAction::Upload,
                        source_info: None,
                        target_info: None,
                        change_details: Default::default(),
                        size_diff: 0,
                        is_large_file: false,
                        requires_chunking: false,
                        requires_encryption: false,
                        priority: 0,
                        estimated_duration_ms: 0,
                        last_processed: None,
                        retry_count: 0,
                        error_message: None,
                        tags: vec![],
                        checksum_type: ChecksumType::Md5,
                        source_checksum: None,
                        target_checksum: None,
                        diff_id: "".to_string(),
                        created_at: SystemTime::now(),
                    });
                }
            }
        }
        Ok(diff)
    }

    async fn sync_file(
        &self,
        source: &dyn StorageProvider,
        target: &dyn StorageProvider,
        file_diff: &FileDiff,
        task: &SyncTask,
        report: &mut SyncReport,
    ) -> Result<(), SyncError> {
        // 检查是否有断点续传记录
        // 查询断点续传记录
        {
            let conn = self.resume_store.lock().unwrap();
            let mut stmt = conn.prepare("SELECT status FROM resume_data WHERE task_id = ?1 AND file_path = ?2 LIMIT 1")?;
            let mut rows = stmt.query(params![task.id, file_diff.path.clone()])?;
            if let Some(row) = rows.next()? {
                let status: String = row.get(0)?;
                if status == "in_progress" {
                    let resume_data = String::new();
                    self.resume_transfer(
                        source,
                        target,
                        file_diff,
                        task,
                        &resume_data,
                        report,
                    ).await;
                    return Ok(());
                }
            }
        }

        // 创建临时文件
        let temp_path = self.create_temp_file()?;

        // 下载文件
        let download_result = source.download(
            &file_diff.path,
            &temp_path,
        ).await?;

        // 加密（如果需要）
        let (encrypted_data, metadata) = if let Some(enc_config) = &task.encryption {
            self.encryption_manager.encrypt_file(
                &temp_path,
                enc_config,
            ).await?
        } else {
            (None, None)
        };

        // 上传文件
        let upload_result = if let Some(encrypted) = encrypted_data {
            // 上传加密文件
            target.upload(&encrypted, &file_diff.path).await?
        } else {
            // 上传原始文件
            target.upload(&temp_path, &file_diff.path).await?
        };

        // 记录成功
        report.add_success(&file_diff.path, file_diff.size_diff);

        // 清理临时文件
        self.cleanup_temp_file(&temp_path)?;

        Ok(())
    }

    async fn resume_transfer(&self,
                             source_storage_provider: &dyn StorageProvider,
                             target_storage_provider: &dyn StorageProvider,
                             file_diff: &FileDiff,
                             task: &SyncTask,
                             data: &String,
                             reporter: &mut SyncReport,
    ) {
        todo!()
    }
}
