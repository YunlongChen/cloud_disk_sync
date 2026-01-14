use crate::config::{DiffMode, SyncTask};
use crate::encryption::EncryptionManager;
use crate::error::{ProviderError, SyncError};
use crate::providers::StorageProvider;
use crate::report::SyncReport;
use crate::sync::diff::{ChecksumType, DiffAction, DiffResult, FileDiff};
use dashmap::DashMap;
use rusqlite::Connection;
use std::collections::HashMap;
use std::time::SystemTime;

pub struct SyncEngine {
    providers: HashMap<String, Box<dyn StorageProvider>>,
    encryption_manager: EncryptionManager,
    diff_cache: DashMap<String, FileDiff>,
    resume_store: HashMap<String, String>,
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
            resume_store: todo!(),
        })
    }

    pub async fn sync(&mut self, task: &SyncTask) -> Result<SyncReport, SyncError> {
        let mut report = SyncReport::new();
        report.task_id = task.id.clone();

        let source_provider = self.get_provider(&task.source_account)
            .ok_or(SyncError::Provider(task.source_account.clone()))?;

        let target_provider = self.get_provider(&task.target_account)
            .ok_or(SyncError::Provider(ProviderError::NotFound("")))?;

        // 计算文件差异
        let diff = self.calculate_diff(
            source_provider.as_ref(),
            target_provider.as_ref(),
            &task.source_path,
            &task.target_path,
            &task.diff_mode,
        ).await?;

        // 执行同步
        for file_diff in diff.files {
            match file_diff.action {
                DiffAction::Upload => {
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
                    report.add_conflict(file_diff.path);
                }
                _ => {}
            }
        }

        // 验证完整性
        if task.verify_integrity {
            self.verify_integrity(
                source_provider.as_ref(),
                target_provider.as_ref(),
                &diff,
            ).await?;
        }

        Ok(report)
    }

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
                    if source_info.hash != target_info.hash {
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
                        created_at: (),
                    });
                }
            }
        }
        Ok(diff)
    }

    async fn sync_file(
        &mut self,
        source: &dyn StorageProvider,
        target: &dyn StorageProvider,
        file_diff: &FileDiff,
        task: &SyncTask,
        report: &mut SyncReport,
    ) -> Result<(), SyncError> {
        // 检查是否有断点续传记录
        let resume_key = format!("{}_{}", task.id, file_diff.path);
        match self.resume_store.get(resume_key.as_bytes()) {
            Ok(Some(resume_data)) => {
                // 从断点继续
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
            _ => {
                todo!()
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