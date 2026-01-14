use crate::config::{AccountConfig, EncryptionConfig, SyncTask};
use crate::core::audit::{AuditFilter, AuditOperation};
use crate::core::health::{ConnectivityStatus, HealthStatus, StorageHealth};
use crate::core::resources::{DiskHandle, MemoryHandle, ResourceLimits, ResourceUsage};
use crate::core::scheduler::ScheduledTask;
use crate::error::{Result, SyncError};
use crate::plugins::hooks::PluginHook;
use crate::report::SyncReport;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 状态管理 trait
pub trait StateManager: Send + Sync {
    fn save_state(&self, key: &str, value: &[u8]) -> Result<()>;
    fn load_state(&self, key: &str) -> Result<Option<Vec<u8>>>;
    fn delete_state(&self, key: &str) -> Result<()>;
    fn list_states(&self, prefix: &str) -> Result<Vec<String>>;
}

/// 进度报告 trait
pub trait ProgressReporter: Send + Sync {
    fn report_start(&self, total_size: u64, total_files: usize);
    fn report_progress(&self, current_size: u64, current_file: &str);
    fn report_file_complete(&self, file: &str, size: u64, duration: Duration);
    fn report_error(&self, file: &str, error: &SyncError);
    fn report_complete(&self, stats: &TransferStats);
}

/// 重试策略 trait
pub trait RetryStrategy: Send + Sync {
    fn should_retry(&self, attempt: u32, error: &SyncError) -> bool;
    fn delay_before_retry(&self, attempt: u32) -> Duration;
    fn max_attempts(&self) -> u32;
}

/// 流控策略 trait
pub trait RateLimiter: Send + Sync {
    async fn acquire<'a>(&'a self) -> Result<()>
    where
        Self: 'a;

    fn current_rate(&self) -> f64; // 请求/秒
    fn set_rate(&self, requests_per_second: f64);
    fn try_acquire(&self) -> bool;
}

/// 校验和计算 trait
pub trait ChecksumCalculator: Send + Sync {
    fn calculate_file_checksum(&self, path: &Path) -> Result<String>;
    fn calculate_data_checksum(&self, data: &[u8]) -> String;
    fn verify_checksum(&self, path: &Path, expected: &str) -> Result<bool>;
}

/// 文件差异检测 trait
pub trait DiffDetector: Send + Sync {
    fn detect_changes(
        &self,
        source_files: &[FileMetadata],
        target_files: &[FileMetadata],
        options: &DiffOptions,
    ) -> Result<Vec<FileChange>>;

    fn calculate_diff_size(&self, changes: &[FileChange]) -> u64;
}

/// 文件过滤器 trait
pub trait FileFilter: Send + Sync {
    fn should_include(&self, file: &FileMetadata) -> bool;
    fn filter_files(&self, files: &[FileMetadata]) -> Vec<FileMetadata>;
}

/// 任务调度器 trait
#[async_trait]
pub trait TaskScheduler: Send + Sync {
    async fn schedule(&self, task: ScheduledTask) -> Result<String>;
    async fn cancel(&self, task_id: &str) -> Result<()>;
    async fn list_scheduled(&self) -> Result<Vec<ScheduledTask>>;
    async fn trigger_now(&self, task_id: &str) -> Result<()>;
}

/// 通知器 trait
pub trait Notifier: Send + Sync {
    fn notify_success(&self, report: &SyncReport);
    fn notify_error(&self, error: &SyncError);
    fn notify_warning(&self, warning: &str);
    fn notify_progress(&self, progress: &ProgressUpdate);
}

/// 审计记录器 trait
pub trait AuditLogger: Send + Sync {
    fn log_operation(&self, operation: AuditOperation);
    fn query_operations(
        &self,
        filter: AuditFilter,
        limit: Option<usize>,
    ) -> Result<Vec<AuditOperation>>;
}

/// 配置验证器 trait
pub trait ConfigValidator: Send + Sync {
    fn validate_account(&self, account: &AccountConfig) -> Result<()>;
    fn validate_task(&self, task: &SyncTask) -> Result<()>;
    fn validate_encryption(&self, config: &EncryptionConfig) -> Result<()>;
}

/// 健康检查 trait
pub trait HealthChecker: Send + Sync {
    async fn check_provider_health(&self, provider_id: &str) -> Result<HealthStatus>;
    async fn check_storage_health(&self) -> Result<StorageHealth>;
    async fn check_connectivity(&self) -> Result<ConnectivityStatus>;
}

/// 资源管理 trait
pub trait ResourceManager: Send + Sync {
    fn allocate_memory(&self, size: usize) -> Result<MemoryHandle>;
    fn allocate_disk(&self, size: u64) -> Result<DiskHandle>;
    fn current_usage(&self) -> ResourceUsage;
    fn set_limits(&self, limits: ResourceLimits);
}

/// 插件系统 trait
#[async_trait]
pub trait Plugin: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn description(&self) -> &str;

    async fn initialize(&self) -> Result<()>;
    async fn shutdown(&self) -> Result<()>;

    fn hooks(&self) -> Vec<PluginHook>;
}

/// 钩子处理器 trait
#[async_trait]
pub trait HookHandler: Send + Sync {
    async fn on_before_sync(&self, task: &SyncTask) -> Result<()>;
    async fn on_after_sync(&self, task: &SyncTask, report: &SyncReport) -> Result<()>;
    async fn on_file_uploading(&self, file: &FileMetadata) -> Result<()>;
    async fn on_file_uploaded(&self, file: &FileMetadata) -> Result<()>;
    async fn on_error(&self, error: &SyncError) -> Result<()>;
}

// 数据传输统计
#[derive(Debug, Clone)]
pub struct TransferStats {
    pub total_files: usize,
    pub successful_files: usize,
    pub failed_files: usize,
    pub skipped_files: usize,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub average_speed: f64, // bytes/second
    pub total_duration: Duration,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub end_time: chrono::DateTime<chrono::Utc>,
}

// 文件元数据
#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub path: PathBuf,
    pub size: u64,
    pub modified: i64,
    pub created: i64,
    pub accessed: i64,
    pub checksum: Option<String>,
    pub permissions: u32,
    pub is_dir: bool,
    pub is_symlink: bool,
}

// 文件变化
#[derive(Debug, Clone)]
pub enum FileChange {
    Added(FileMetadata),
    Modified { old: FileMetadata, new: FileMetadata },
    Deleted(FileMetadata),
    Moved { from: FileMetadata, to: FileMetadata },
    Unchanged(FileMetadata),
}

// 差异检测选项
#[derive(Debug, Clone)]
pub struct DiffOptions {
    pub compare_size: bool,
    pub compare_mtime: bool,
    pub compare_checksum: bool,
    pub ignore_patterns: Vec<String>,
    pub max_depth: Option<usize>,
    pub follow_symlinks: bool,
}

// 进度更新
#[derive(Debug, Clone)]
pub struct ProgressUpdate {
    pub current_file: String,
    pub current_size: u64,
    pub total_size: u64,
    pub current_file_index: usize,
    pub total_files: usize,
    pub speed: f64, // bytes/second
    pub elapsed: Duration,
    pub estimated_remaining: Duration,
}

