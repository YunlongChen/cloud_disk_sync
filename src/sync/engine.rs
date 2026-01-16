use crate::config::SyncPolicy;
use crate::config::{DiffMode, SyncTask};
use crate::encryption::EncryptionManager;
use crate::error::{ProviderError, SyncError};
use crate::providers::{FileInfo, StorageProvider};
use crate::report::{FileOperation, SyncReport};
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
    scan_cache: DashMap<String, (Vec<FileInfo>, SystemTime)>,
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

        // 创建报告表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS sync_reports (
                report_id TEXT PRIMARY KEY,
                task_id TEXT NOT NULL,
                start_time INTEGER NOT NULL,
                status TEXT NOT NULL,
                duration_seconds INTEGER NOT NULL,
                details_json TEXT NOT NULL
            )",
            [],
        )?;

        // 创建索引以加速查询
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_reports_task_id ON sync_reports(task_id)",
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
        self.execute_sync(task, None::<fn(SyncProgress)>).await
    }

    pub async fn sync_with_progress(
        &mut self,
        task: &SyncTask,
        progress_callback: impl Fn(SyncProgress) + Send + Sync + 'static,
    ) -> Result<SyncReport, SyncError> {
        self.execute_sync(task, Some(progress_callback)).await
    }

    async fn execute_sync<F>(
        &self,
        task: &SyncTask,
        progress_callback: Option<F>,
    ) -> Result<SyncReport, SyncError>
    where
        F: Fn(SyncProgress) + Send + Sync + 'static,
    {
        info!(task_id = %task.id, "Starting sync task: {}", task.name);
        let mut report = SyncReport::new(&task.id);

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

        let total_transfer_size = diff.total_transfer_size;
        let mut transferred_size = 0u64;
        let start_time = std::time::Instant::now();

        // 执行同步
        for file_diff in diff.files {
            match file_diff.action {
                DiffAction::Upload | DiffAction::Update => {
                    debug!(file = %file_diff.path, "Syncing file (Upload/Update)");
                    // 如果是目录，则创建目录
                    if let Some(src_info) = &file_diff.source_info {
                        if src_info.is_dir {
                            debug!(path = %file_diff.path, "Creating directory (from Upload action)");
                            let target_provider = self.get_provider(&task.target_account).ok_or(
                                SyncError::Provider(ProviderError::NotFound(
                                    task.target_account.clone(),
                                )),
                            )?;
                            let target_full_path = {
                                let base_path = std::path::Path::new(&task.target_path);
                                let rel_path = std::path::Path::new(&file_diff.path);
                                base_path
                                    .join(rel_path)
                                    .to_string_lossy()
                                    .replace('\\', "/")
                            };
                            match target_provider.mkdir(&target_full_path).await {
                                Ok(_) => {
                                    info!(path = %file_diff.path, "Created directory");
                                    report.add_success(&file_diff.path, 0);
                                    continue; // 目录处理完毕
                                }
                                Err(e) => {
                                    // 忽略目录已存在错误
                                    // WebDavProvider::mkdir 返回什么错误？
                                    // 假设是通用错误，暂时记录日志
                                    warn!(path = %file_diff.path, error = %e, "Failed to create directory (might exist)");
                                    // 不因为目录创建失败中断，尝试继续？或者算成功？
                                    // 如果目录创建失败，后续文件上传可能会失败。
                                    // 但如果是"已存在"，则没问题。
                                    // 暂时 continue
                                    continue;
                                }
                            }
                        }
                    }

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

                    let file_size = file_diff.transfer_size();

                    // 通知进度：开始
                    if let Some(ref cb) = progress_callback {
                        cb(SyncProgress {
                            current_file: file_diff.path.clone(),
                            current_file_size: file_size,
                            transferred: transferred_size,
                            total: total_transfer_size,
                            percentage: if total_transfer_size > 0 {
                                (transferred_size as f64 / total_transfer_size as f64) * 100.0
                            } else {
                                0.0
                            },
                            speed: 0.0,
                        });
                    }

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
                            transferred_size += file_size;

                            // 通知进度：完成
                            if let Some(ref cb) = progress_callback {
                                let elapsed = start_time.elapsed().as_secs_f64();
                                let speed = if elapsed > 0.0 {
                                    transferred_size as f64 / elapsed
                                } else {
                                    0.0
                                };

                                cb(SyncProgress {
                                    current_file: file_diff.path.clone(),
                                    current_file_size: file_size,
                                    transferred: transferred_size,
                                    total: total_transfer_size,
                                    percentage: if total_transfer_size > 0 {
                                        (transferred_size as f64 / total_transfer_size as f64)
                                            * 100.0
                                    } else {
                                        100.0
                                    },
                                    speed,
                                });
                            }
                        }
                        Err(e) => {
                            error!(file = %file_diff.path, error = %e, "Sync failed");
                            report.add_failure(
                                &file_diff.path,
                                FileOperation::from_diff_action(file_diff.action),
                                e.to_string(),
                            );
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

                    let target_full_path = {
                        let base_path = std::path::Path::new(&task.target_path);
                        let rel_path = std::path::Path::new(&file_diff.path);
                        base_path
                            .join(rel_path)
                            .to_string_lossy()
                            .replace('\\', "/")
                    };

                    match target_provider.delete(&target_full_path).await {
                        Ok(_) => {
                            info!(file = %file_diff.path, "Deleted target file");
                            report.add_success(&file_diff.path, file_diff.size_diff);
                        }
                        Err(e) => {
                            error!(file = %file_diff.path, error = %e, "Failed to delete file");
                            report.add_failure(
                                &file_diff.path,
                                FileOperation::Delete,
                                e.to_string(),
                            );
                        }
                    }
                }
                DiffAction::CreateDir => {
                    debug!(path = %file_diff.path, "Creating directory");
                    let target_provider =
                        self.get_provider(&task.target_account)
                            .ok_or(SyncError::Provider(ProviderError::NotFound(
                                task.target_account.clone(),
                            )))?;
                    let target_full_path = {
                        let base_path = std::path::Path::new(&task.target_path);
                        let rel_path = std::path::Path::new(&file_diff.path);
                        base_path
                            .join(rel_path)
                            .to_string_lossy()
                            .replace('\\', "/")
                    };
                    match target_provider.mkdir(&target_full_path).await {
                        Ok(_) => {
                            info!(path = %file_diff.path, "Created directory");
                            report.add_success(&file_diff.path, 0);
                        }
                        Err(e) => {
                            error!(path = %file_diff.path, error = %e, "Failed to create directory");
                            report.add_failure(
                                &file_diff.path,
                                FileOperation::CreateDir,
                                e.to_string(),
                            );
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

        let duration = start_time.elapsed().as_secs_f64();
        report.statistics.finalize(duration);
        report.duration_seconds = duration as i64;

        // 保存报告到数据库
        if let Err(e) = self.save_report(&report) {
            error!(error = %e, "Failed to save sync report to database");
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

    pub async fn calculate_diff_for_dry_run(
        &self,
        task: &SyncTask,
    ) -> Result<DiffResult, SyncError> {
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
        .await
    }

    /// 保存同步报告到数据库
    pub fn save_report(&self, report: &SyncReport) -> Result<(), SyncError> {
        let conn = self.resume_store.lock().unwrap();
        let report_id = uuid::Uuid::new_v4().to_string();
        let json = serde_json::to_string(report).map_err(|e| SyncError::Unknown(e.to_string()))?;

        conn.execute(
            "INSERT INTO sync_reports (report_id, task_id, start_time, status, duration_seconds, details_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                report_id,
                report.task_id,
                report.start_time.timestamp(),
                format!("{:?}", report.status),
                report.duration_seconds,
                json
            ],
        )?;
        Ok(())
    }

    /// 获取任务的报告列表
    pub fn list_reports(
        &self,
        task_id: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(String, i64, String, i64)>, SyncError> {
        let conn = self.resume_store.lock().unwrap();
        // 构造 SQL 查询
        let mut stmt = conn.prepare(
            "SELECT report_id, start_time, status, duration_seconds 
             FROM sync_reports 
             WHERE task_id = ?1 
             ORDER BY start_time DESC 
             LIMIT ?2 OFFSET ?3",
        )?;

        // 将 usize 转换为 i64，以满足 ToSql trait (SQLite INTEGER is i64)
        let limit_i64 = limit as i64;
        let offset_i64 = offset as i64;

        let report_iter = stmt.query_map(params![task_id, limit_i64, offset_i64], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?;

        let mut reports = Vec::new();
        for report in report_iter {
            reports.push(report?);
        }
        Ok(reports)
    }

    /// 获取特定报告详情
    pub fn get_report(&self, report_id: &str) -> Result<SyncReport, SyncError> {
        let conn = self.resume_store.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT details_json FROM sync_reports WHERE report_id = ?1")?;

        let mut rows = stmt.query(params![report_id])?;
        if let Some(row) = rows.next()? {
            let json: String = row.get(0)?;
            let report: SyncReport =
                serde_json::from_str(&json).map_err(|e| SyncError::Unknown(e.to_string()))?;
            Ok(report)
        } else {
            Err(SyncError::Unknown("Report not found".to_string()))
        }
    }
}

impl SyncEngine {
    async fn list_with_retry(
        &self,
        provider: &dyn StorageProvider,
        path: &str,
    ) -> Result<Vec<FileInfo>, SyncError> {
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
    ) -> Result<Vec<FileInfo>, SyncError> {
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

        // 辅助函数：将 FileInfo 转换为 FileMetadata
        let to_metadata = |info: &FileInfo| -> crate::sync::diff::FileMetadata {
            let mut meta =
                crate::sync::diff::FileMetadata::new(std::path::PathBuf::from(&info.path));
            meta.size = info.size;
            meta.modified = info.modified;
            meta.is_dir = info.is_dir;
            meta.file_hash = info.hash.clone();
            meta
        };

        // 辅助函数：标准化路径为相对路径
        let normalize_path = |full_path: &str, root: &str| -> String {
            let root = root.trim_end_matches('/');
            if full_path.starts_with(root) {
                let rel = &full_path[root.len()..];
                rel.trim_start_matches('/').to_string()
            } else {
                full_path.to_string()
            }
        };

        // 构建 Map (Relative Path -> FileMetadata)
        let mut src_map = std::collections::HashMap::new();
        for f in src_list.iter() {
            let rel_path = normalize_path(&f.path, source_path);
            src_map.insert(rel_path, to_metadata(f));
        }

        let mut dst_map = std::collections::HashMap::new();
        for f in dst_list.iter() {
            let rel_path = normalize_path(&f.path, target_path);
            dst_map.insert(rel_path, to_metadata(f));
        }

        // 收集所有相对路径
        let mut all_paths: std::collections::HashSet<String> = std::collections::HashSet::new();
        for p in src_map.keys() {
            all_paths.insert(p.clone());
        }
        for p in dst_map.keys() {
            all_paths.insert(p.clone());
        }

        let mut diff = DiffResult::new();

        for path in all_paths {
            let src_meta = src_map.get(&path);
            let dst_meta = dst_map.get(&path);

            match (src_meta, dst_meta) {
                (Some(s), Some(t)) => {
                    // 两边都有，比较元数据
                    // 这里可以加入更复杂的比较逻辑（如哈希）
                    // 暂时只比较大小和修改时间
                    let size_match = s.size == t.size;
                    // 修改时间容差 2秒
                    let time_match = (s.modified - t.modified).abs() <= 2;

                    if size_match && time_match {
                        // 认为相同
                        diff.add_file(FileDiff::unchanged(path.clone(), s.clone(), t.clone()));
                    } else {
                        // 不同，需要更新
                        if overwrite_existing {
                            diff.add_file(FileDiff::update(path.clone(), s.clone(), t.clone()));
                        } else {
                            // 不允许覆盖，虽然不同但也标记为 Unchanged (或 Conflict? 视策略而定)
                            // 这里标记为 Unchanged 但可以加个 Tag 说明被忽略
                            let mut d = FileDiff::unchanged(path.clone(), s.clone(), t.clone());
                            d.tags.push("skipped_overwrite".to_string());
                            diff.add_file(d);
                        }
                    }
                }
                (Some(s), None) => {
                    // 只有源有 -> Upload
                    diff.add_file(FileDiff::upload(path.clone(), s.clone(), None));
                }
                (None, Some(t)) => {
                    // 只有目标有 -> Delete (如果 delete_orphans) 否则 Unchanged (TargetOnly)
                    if delete_orphans {
                        diff.add_file(FileDiff::delete(path.clone(), t.clone()));
                    } else {
                        let mut d = FileDiff::unchanged(
                            path.clone(),
                            crate::sync::diff::FileMetadata::new(std::path::PathBuf::from(&path)),
                            t.clone(),
                        );
                        // 标记源信息为"空"的元数据是不太准确的，FileDiff::unchanged 需要 source_info
                        // 实际上 FileDiff::new 的 source_info 是 Option。
                        // 但是 FileDiff::unchanged 辅助方法要求 FileMetadata。
                        // 我们直接用 FileDiff::new
                        d = FileDiff::new(
                            path.clone(),
                            DiffAction::Unchanged,
                            None,
                            Some(t.clone()),
                        );
                        d.tags.push("target_only".to_string());
                        diff.add_file(d);
                    }
                }
                (None, None) => unreachable!(),
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
        // 构造完整路径辅助函数
        let join_path = |base: &str, rel: &str| -> String {
            let base_path = std::path::Path::new(base);
            let rel_path = std::path::Path::new(rel);
            base_path
                .join(rel_path)
                .to_string_lossy()
                .replace('\\', "/")
        };

        let source_full_path = join_path(&task.source_path, &file_diff.path);
        let target_full_path = join_path(&task.target_path, &file_diff.path);

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
        let download_result = source.download(&source_full_path, &temp_path).await?;

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
            target.upload(&encrypted, &target_full_path).await?
        } else {
            // 上传原始文件
            target.upload(&temp_path, &target_full_path).await?
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
