use crate::config::SyncPolicy;
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
use tokio::time::{Duration, sleep};
use tracing::{debug, error, info, instrument, warn};

pub struct SyncEngine {
    providers: HashMap<String, Box<dyn StorageProvider>>,
    encryption_manager: EncryptionManager,
    diff_cache: DashMap<String, FileDiff>,
    resume_store: Arc<Mutex<Connection>>,
    /// 扫描缓存：key -> (列表快照, 上次扫描时间)
    scan_cache: DashMap<String, (Vec<crate::providers::FileInfo>, SystemTime)>,
}

impl SyncEngine {
    pub async fn new() -> Result<Self, SyncError> {
        let db_path = dirs::data_dir()
            .ok_or(SyncError::Unknown(String::from(
                "Failed to obtain data_dir",
            )))?
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
            scan_cache: DashMap::new(),
        })
    }

    /// 注册存储提供器到引擎
    pub fn register_provider(&mut self, account_id: String, provider: Box<dyn StorageProvider>) {
        self.providers.insert(account_id, provider);
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
        info!(task_id = %task.id, "Starting sync task: {}", task.name);
        let mut report = SyncReport::new();
        report.task_id = task.id.clone();

        // 计算文件差异（限定借用作用域）
        let diff = {
            let source_provider =
                self.get_provider(&task.source_account)
                    .ok_or(SyncError::Provider(ProviderError::NotFound(
                        task.source_account.clone(),
                    )))?;
            let target_provider =
                self.get_provider(&task.target_account)
                    .ok_or(SyncError::Provider(ProviderError::NotFound(
                        task.target_account.clone(),
                    )))?;
            self.calculate_diff(
                source_provider.as_ref(),
                target_provider.as_ref(),
                &task.source_path,
                &task.target_path,
                &task.diff_mode,
                task.sync_policy.as_ref(),
                &format!("{}::{}", task.source_account, task.source_path),
                &format!("{}::{}", task.target_account, task.target_path),
            )
            .await?
        };

        info!(task_id = %task.id, total_files = diff.files.len(), "Diff calculation completed");

        // 执行同步
        for file_diff in diff.files {
            match file_diff.action {
                DiffAction::Upload => {
                    debug!(file = %file_diff.path, "Syncing file (Upload)");
                    // 重新获取 provider，避免与 &self 的可变借用冲突
                    let source_provider =
                        self.get_provider(&task.source_account)
                            .ok_or(SyncError::Provider(ProviderError::NotFound(
                                task.source_account.clone(),
                            )))?;
                    let target_provider =
                        self.get_provider(&task.target_account)
                            .ok_or(SyncError::Provider(ProviderError::NotFound(
                                task.target_account.clone(),
                            )))?;

                    match self
                        .sync_file(
                            source_provider.as_ref(),
                            target_provider.as_ref(),
                            &file_diff,
                            task,
                            &mut report,
                        )
                        .await
                    {
                        Ok(_) => {
                            debug!(file = %file_diff.path, "Sync successful");
                        }
                        Err(e) => {
                            error!(file = %file_diff.path, error = %e, "Sync failed");
                            report
                                .errors
                                .push(format!("Failed to sync {}: {}", file_diff.path, e));
                            report.statistics.files_failed += 1;
                        }
                    }
                }
                DiffAction::Download => {
                    // 反向同步
                }
                DiffAction::Delete => {
                    debug!(file = %file_diff.path, "Deleting target file");
                    // 删除目标文件
                    let target_provider =
                        self.get_provider(&task.target_account)
                            .ok_or(SyncError::Provider(ProviderError::NotFound(
                                task.target_account.clone(),
                            )))?;

                    match target_provider.delete(&file_diff.path).await {
                        Ok(_) => {
                            info!(file = %file_diff.path, "Deleted target file");
                            report.add_success(&file_diff.path, file_diff.size_diff);
                        }
                        Err(e) => {
                            error!(file = %file_diff.path, error = %e, "Failed to delete file");
                            report
                                .errors
                                .push(format!("Failed to delete {}: {}", file_diff.path, e));
                            report.statistics.files_failed += 1;
                        }
                    }
                }
                DiffAction::Conflict => {
                    warn!(file = %file_diff.path, "Conflict detected");
                    report.add_conflict(&file_diff.path);
                }
                _ => {}
            }
        }
        info!(task_id = %task.id, stats = ?report.statistics, "Sync task completed");
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
        let source_provider =
            self.get_provider(&task.source_account)
                .ok_or(SyncError::Provider(ProviderError::NotFound(
                    task.source_account.clone(),
                )))?;
        let target_provider =
            self.get_provider(&task.target_account)
                .ok_or(SyncError::Provider(ProviderError::NotFound(
                    task.target_account.clone(),
                )))?;

        let mut result = VerificationResult::new();
        let source_files = self
            .walk_directory(source_provider.as_ref(), &task.source_path)
            .await?;
        let target_files = self
            .walk_directory(target_provider.as_ref(), &task.target_path)
            .await?;

        for (path, source_info) in &source_files {
            progress_callback(VerificationProgress {
                current_path: path.clone(),
                current_file: result.checked_files + 1,
                total_files: source_files.len(),
            });
            if let Some(target_info) = target_files.get(path) {
                if let (Some(source_hash), Some(target_hash)) =
                    (&source_info.file_hash, &target_info.file_hash)
                {
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
    async fn list_with_retry(
        &self,
        provider: &dyn StorageProvider,
        path: &str,
    ) -> Result<Vec<crate::providers::FileInfo>, SyncError> {
        let max_retries = 3;
        let mut last_error = SyncError::Unknown("Initial".to_string());

        for i in 0..=max_retries {
            match provider.list(path).await {
                Ok(list) => return Ok(list),
                Err(e) => {
                    last_error = e;
                    if i < max_retries {
                        sleep(Duration::from_millis(100 * (1 << i))).await;
                    }
                }
            }
        }
        Err(last_error)
    }

    async fn recursive_list(
        &self,
        provider: &dyn StorageProvider,
        root: &str,
    ) -> Result<Vec<crate::providers::FileInfo>, SyncError> {
        let mut result = Vec::new();
        let mut stack = vec![root.to_string()];

        while let Some(dir) = stack.pop() {
            // list_with_retry might fail for deep directories if we hit limits, but we have retry now.
            let entries = self.list_with_retry(provider, &dir).await?;
            for entry in entries {
                if entry.is_dir {
                    // Ensure we don't get into infinite loop if provider returns "." or ".."
                    // WebDavProvider usually filters them or returns absolute paths.
                    // Also avoid re-listing the dir itself if it's returned.
                    // WebDavProvider::parse_propfind_response checks `path == base_path` and skips it.
                    // So we are safe from self-inclusion.
                    stack.push(entry.path.clone());
                }
                result.push(entry);
            }
        }
        Ok(result)
    }

    /// 计算差异，支持扫描结果缓存与限频
    async fn calculate_diff(
        &self,
        source: &dyn StorageProvider,
        target: &dyn StorageProvider,
        source_path: &str,
        target_path: &str,
        diff_mode: &DiffMode,
        policy: Option<&SyncPolicy>,
        source_key: &str,
        target_key: &str,
    ) -> Result<DiffResult, SyncError> {
        debug!(source = source_path, target = target_path, mode = ?diff_mode, "Calculating diff");
        // 读取策略
        let (delete_orphans, overwrite_existing, cooldown_secs) = if let Some(p) = policy {
            (p.delete_orphans, p.overwrite_existing, p.scan_cooldown_secs)
        } else {
            // 默认策略：删除孤立、允许覆盖、不开启限频
            (true, true, 0u64)
        };

        // 根据 DiffMode 与策略决定是否使用缓存
        let use_cache = matches!(diff_mode, DiffMode::Smart) && cooldown_secs > 0;
        let now = SystemTime::now();

        // 获取源列表（考虑缓存）
        let src_list = if use_cache {
            if let Some((cached, ts)) = self.scan_cache.get(source_key).map(|v| v.clone()) {
                if now.duration_since(ts).unwrap_or_default().as_secs() < cooldown_secs {
                    debug!(key = source_key, "Using cached source list");
                    cached
                } else {
                    debug!(key = source_key, "Refreshing source list (cache expired)");
                    let fresh = self.recursive_list(source, source_path).await?;
                    self.scan_cache
                        .insert(source_key.to_string(), (fresh.clone(), now));
                    fresh
                }
            } else {
                debug!(key = source_key, "Fetching source list (no cache)");
                let fresh = self.recursive_list(source, source_path).await?;
                self.scan_cache
                    .insert(source_key.to_string(), (fresh.clone(), now));
                fresh
            }
        } else {
            debug!(key = source_key, "Fetching source list (cache disabled)");
            let fresh = self.recursive_list(source, source_path).await?;
            self.scan_cache
                .insert(source_key.to_string(), (fresh.clone(), now));
            fresh
        };

        // 获取目标列表（考虑缓存）
        let dst_list = if use_cache {
            if let Some((cached, ts)) = self.scan_cache.get(target_key).map(|v| v.clone()) {
                if now.duration_since(ts).unwrap_or_default().as_secs() < cooldown_secs {
                    debug!(key = target_key, "Using cached target list");
                    cached
                } else {
                    debug!(key = target_key, "Refreshing target list (cache expired)");
                    let fresh = self.recursive_list(target, target_path).await?;
                    self.scan_cache
                        .insert(target_key.to_string(), (fresh.clone(), now));
                    fresh
                }
            } else {
                debug!(key = target_key, "Fetching target list (no cache)");
                let fresh = self.recursive_list(target, target_path).await?;
                self.scan_cache
                    .insert(target_key.to_string(), (fresh.clone(), now));
                fresh
            }
        } else {
            debug!(key = target_key, "Fetching target list (cache disabled)");
            let fresh = self.recursive_list(target, target_path).await?;
            self.scan_cache
                .insert(target_key.to_string(), (fresh.clone(), now));
            fresh
        };

        // 构建集合（仅文件，不含目录）
        let mut src_set = std::collections::HashSet::new();
        let mut dst_set = std::collections::HashSet::new();

        for f in src_list.iter().filter(|f| !f.is_dir) {
            // 统一路径到目标根目录（假设两端根目录一致）
            let p = if f.path.starts_with('/') {
                f.path.clone()
            } else {
                format!("{}/{}", source_path.trim_end_matches('/'), f.path)
            };
            src_set.insert(p);
        }
        for f in dst_list.iter().filter(|f| !f.is_dir) {
            let p = if f.path.starts_with('/') {
                f.path.clone()
            } else {
                format!("{}/{}", target_path.trim_end_matches('/'), f.path)
            };
            dst_set.insert(p);
        }

        let mut diff = DiffResult::new();

        // 需要上传（包括覆盖）：源存在
        for p in src_set.iter() {
            let source_meta = crate::sync::diff::FileMetadata::new(std::path::PathBuf::from(p));
            let target_exists = dst_set.contains(p);
            // 若不允许覆盖且目标存在，则跳过
            if !overwrite_existing && target_exists {
                continue;
            }
            let target_meta = if target_exists {
                Some(crate::sync::diff::FileMetadata::new(
                    std::path::PathBuf::from(p),
                ))
            } else {
                None
            };
            let d = crate::sync::diff::FileDiff::upload(p.clone(), source_meta, target_meta);
            diff.add_file(d);
        }

        // 需要删除：仅目标存在
        if delete_orphans {
            for p in dst_set.difference(&src_set) {
                let target_meta = crate::sync::diff::FileMetadata::new(std::path::PathBuf::from(p));
                let d = crate::sync::diff::FileDiff::delete(p.clone(), target_meta);
                diff.add_file(d);
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
            let mut stmt = conn.prepare(
                "SELECT status FROM resume_data WHERE task_id = ?1 AND file_path = ?2 LIMIT 1",
            )?;
            let mut rows = stmt.query(params![task.id, file_diff.path.clone()])?;
            if let Some(row) = rows.next()? {
                let status: String = row.get(0)?;
                if status == "in_progress" {
                    let resume_data = String::new();
                    self.resume_transfer(source, target, file_diff, task, &resume_data, report)
                        .await;
                    return Ok(());
                }
            }
        }

        // 创建临时文件
        let temp_path = self.create_temp_file()?;

        // 下载文件
        let download_result = source.download(&file_diff.path, &temp_path).await?;

        // 加密（如果需要）
        let (encrypted_data, metadata) = if let Some(enc_config) = &task.encryption {
            self.encryption_manager
                .encrypt_file(&temp_path, enc_config)
                .await?
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

    async fn resume_transfer(
        &self,
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
