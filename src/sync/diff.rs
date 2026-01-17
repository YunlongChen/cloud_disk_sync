// src/sync/diff.rs
use crate::error::{Result, SyncError};
use crate::utils::format_bytes;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// 文件差异操作类型
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum DiffAction {
    /// 需要上传到目标
    Upload,
    /// 需要从目标下载
    Download,
    /// 需要在目标删除
    Delete,
    /// 冲突需要解决
    Conflict,
    /// 文件移动或重命名
    Move,
    /// 文件更新（内容或元数据变化）
    Update,
    /// 文件未变化
    Unchanged,
    /// 创建目录
    CreateDir,
}

impl DiffAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Upload => "upload",
            Self::Download => "download",
            Self::Delete => "delete",
            Self::Conflict => "conflict",
            Self::Move => "move",
            Self::Update => "update",
            Self::Unchanged => "unchanged",
            Self::CreateDir => "create_dir",
        }
    }

    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Upload => "📤",
            Self::Download => "📥",
            Self::Delete => "🗑️",
            Self::Conflict => "⚠️",
            Self::Move => "📦",
            Self::Update => "🔄",
            Self::Unchanged => "✅",
            Self::CreateDir => "📁",
        }
    }

    pub fn is_transfer(&self) -> bool {
        matches!(self, Self::Upload | Self::Download)
    }

    pub fn is_destructive(&self) -> bool {
        matches!(self, Self::Delete)
    }

    pub fn requires_user_action(&self) -> bool {
        matches!(self, Self::Conflict)
    }
}

/// 文件差异详情
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiff {
    /// 相对路径（相对于同步根目录）
    pub path: String,
    /// 差异操作类型
    pub action: DiffAction,
    /// 源文件信息（如果存在）
    pub source_info: Option<FileMetadata>,
    /// 目标文件信息（如果存在）
    pub target_info: Option<FileMetadata>,
    /// 变化详情
    pub change_details: ChangeDetails,
    /// 文件大小差异（字节）
    pub size_diff: i64,
    /// 是否为大文件（超过阈值）
    pub is_large_file: bool,
    /// 是否需要分块传输
    pub requires_chunking: bool,
    /// 是否需要加密
    pub requires_encryption: bool,
    /// 优先级（0-100，越高越先处理）
    pub priority: u8,
    /// 预计传输时间（毫秒）
    pub estimated_duration_ms: u64,
    /// 上次处理时间
    pub last_processed: Option<SystemTime>,
    /// 重试次数
    pub retry_count: u32,
    /// 错误信息（如果之前处理失败）
    pub error_message: Option<String>,
    /// 自定义标签
    pub tags: Vec<String>,
    /// 校验和类型
    pub checksum_type: ChecksumType,
    /// 源文件校验和
    pub source_checksum: Option<String>,
    /// 目标文件校验和
    pub target_checksum: Option<String>,
    /// 差异ID（用于去重和跟踪）
    pub diff_id: String,
    /// 创建时间
    pub created_at: SystemTime,
}

impl FileDiff {
    pub fn new(
        path: String,
        action: DiffAction,
        source_info: Option<FileMetadata>,
        target_info: Option<FileMetadata>,
    ) -> Self {
        let size_diff = Self::calculate_size_diff(&source_info, &target_info);
        let is_large_file = Self::is_large_file(size_diff);

        Self {
            path,
            action,
            source_info,
            target_info,
            change_details: ChangeDetails::default(),
            size_diff,
            is_large_file,
            requires_chunking: is_large_file,
            requires_encryption: false,
            priority: Self::calculate_priority(action, size_diff),
            estimated_duration_ms: Self::estimate_duration(size_diff, is_large_file),
            last_processed: None,
            retry_count: 0,
            error_message: None,
            tags: Vec::new(),
            checksum_type: ChecksumType::Sha256,
            source_checksum: None,
            target_checksum: None,
            diff_id: Self::generate_diff_id(),
            created_at: SystemTime::now(),
        }
    }

    pub fn upload(
        path: String,
        source_info: FileMetadata,
        target_info: Option<FileMetadata>,
    ) -> Self {
        Self::new(path, DiffAction::Upload, Some(source_info), target_info)
    }

    pub fn download(
        path: String,
        target_info: FileMetadata,
        source_info: Option<FileMetadata>,
    ) -> Self {
        Self::new(path, DiffAction::Download, source_info, Some(target_info))
    }

    pub fn delete(path: String, target_info: FileMetadata) -> Self {
        Self::new(path, DiffAction::Delete, None, Some(target_info))
    }

    pub fn conflict(path: String, source_info: FileMetadata, target_info: FileMetadata) -> Self {
        let mut diff = Self::new(
            path,
            DiffAction::Conflict,
            Some(source_info),
            Some(target_info),
        );
        diff.priority = 100; // 冲突文件最高优先级
        diff
    }

    pub fn update(path: String, source_info: FileMetadata, target_info: FileMetadata) -> Self {
        Self::new(
            path,
            DiffAction::Update,
            Some(source_info),
            Some(target_info),
        )
    }

    pub fn unchanged(path: String, source_info: FileMetadata, target_info: FileMetadata) -> Self {
        Self::new(
            path,
            DiffAction::Unchanged,
            Some(source_info),
            Some(target_info),
        )
    }

    pub fn create_dir(path: String, source_info: FileMetadata) -> Self {
        Self::new(path, DiffAction::CreateDir, Some(source_info), None)
    }

    pub fn move_file(
        from: String,
        to: String,
        source_info: FileMetadata,
        target_info: FileMetadata,
    ) -> Self {
        let mut diff = Self::new(to, DiffAction::Move, Some(source_info), Some(target_info));
        diff.change_details.old_path = Some(from);
        diff
    }

    fn calculate_size_diff(
        source_info: &Option<FileMetadata>,
        target_info: &Option<FileMetadata>,
    ) -> i64 {
        match (source_info, target_info) {
            (Some(src), Some(dst)) => src.size as i64 - dst.size as i64,
            (Some(src), None) => src.size as i64,
            (None, Some(dst)) => -(dst.size as i64),
            (None, None) => 0,
        }
    }

    fn is_large_file(size_diff: i64) -> bool {
        size_diff.abs() > 1024 * 1024 * 100 // 100MB 以上为大文件
    }

    fn calculate_priority(action: DiffAction, size_diff: i64) -> u8 {
        match action {
            DiffAction::Conflict => 100,
            DiffAction::Delete => 90,
            DiffAction::Update if size_diff.abs() < 1024 * 1024 => 80, // 小文件更新
            DiffAction::Upload | DiffAction::Download => {
                // 小文件优先，大文件靠后
                if size_diff.abs() < 1024 * 1024 {
                    70 // 小文件
                } else if size_diff.abs() < 1024 * 1024 * 10 {
                    60 // 中等文件
                } else {
                    50 // 大文件
                }
            }
            DiffAction::Move => 40,
            DiffAction::CreateDir => 75, // 在上传文件之前创建目录
            DiffAction::Unchanged => 10,
            _ => 30,
        }
    }

    fn estimate_duration(size_diff: i64, is_large_file: bool) -> u64 {
        // 假设平均速度 1MB/s
        let bytes_per_second = 1024 * 1024;
        let duration_secs = (size_diff.abs() as f64 / bytes_per_second as f64).ceil() as u64;

        if is_large_file {
            // 大文件增加额外处理时间
            duration_secs * 1000 + 5000
        } else {
            duration_secs * 1000
        }
    }

    fn generate_diff_id() -> String {
        use uuid::Uuid;
        format!("diff_{}", Uuid::new_v4().simple())
    }

    pub fn calculate_similarity(&self) -> f64 {
        // 计算源文件和目标文件的相似度（0.0-1.0）
        match (&self.source_info, &self.target_info) {
            (Some(src), Some(dst)) => {
                if src.size == dst.size {
                    // 大小相同，检查修改时间等其他因素
                    let time_diff = (src.modified - dst.modified).abs();
                    if time_diff < 2 {
                        0.95 // 时间差小于2秒，高度相似
                    } else {
                        0.5 // 时间差较大，中等相似
                    }
                } else {
                    0.1 // 大小不同，低相似度
                }
            }
            _ => 0.0, // 只有一端存在文件，不相似
        }
    }

    pub fn is_similar(&self, threshold: f64) -> bool {
        self.calculate_similarity() >= threshold
    }

    pub fn should_retry(&self, max_retries: u32) -> bool {
        self.retry_count < max_retries
    }

    pub fn mark_retry(&mut self, error: Option<String>) {
        self.retry_count += 1;
        self.error_message = error;
        self.last_processed = Some(SystemTime::now());
    }

    pub fn mark_success(&mut self) {
        self.last_processed = Some(SystemTime::now());
        self.retry_count = 0;
        self.error_message = None;
    }

    pub fn is_expired(&self, timeout: Duration) -> bool {
        if let Some(last_processed) = self.last_processed {
            last_processed.elapsed().unwrap_or_default() > timeout
        } else {
            false
        }
    }

    pub fn total_size(&self) -> u64 {
        match &self.source_info {
            Some(info) => info.size,
            None => 0,
        }
    }

    pub fn transfer_size(&self) -> u64 {
        if self.action.is_transfer() {
            match &self.source_info {
                Some(info) => info.size,
                None => 0,
            }
        } else {
            0
        }
    }

    pub fn human_readable_size(&self) -> String {
        format_bytes(self.total_size())
    }

    pub fn summary(&self) -> String {
        let action_emoji = self.action.emoji();
        let size_str = self.human_readable_size();

        match self.action {
            DiffAction::Upload => format!("{} 上传: {} ({})", action_emoji, self.path, size_str),
            DiffAction::Download => format!("{} 下载: {} ({})", action_emoji, self.path, size_str),
            DiffAction::Delete => format!("{} 删除: {}", action_emoji, self.path),
            DiffAction::Conflict => format!("{} 冲突: {}", action_emoji, self.path),
            DiffAction::Move => {
                if let Some(old_path) = &self.change_details.old_path {
                    format!("{} 移动: {} -> {}", action_emoji, old_path, self.path)
                } else {
                    format!("{} 移动: {}", action_emoji, self.path)
                }
            }
            DiffAction::Update => format!("{} 更新: {} ({})", action_emoji, self.path, size_str),
            DiffAction::CreateDir => format!("{} 创建目录: {}", action_emoji, self.path),
            DiffAction::Unchanged => format!("{} 未变: {}", action_emoji, self.path),
        }
    }

    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| crate::error::SyncError::Serialization(e.into()))
    }

    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(|e| crate::error::SyncError::Serialization(e.into()))
    }

    pub fn is_encrypted(&self) -> bool {
        self.source_info
            .as_ref()
            .map_or(false, |info| info.is_encrypted)
            || self
                .target_info
                .as_ref()
                .map_or(false, |info| info.is_encrypted)
    }

    pub fn requires_decryption(&self) -> bool {
        self.requires_encryption || self.is_encrypted()
    }
}

/// 文件元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub path: PathBuf,
    pub size: u64,
    pub modified: i64,
    pub created: i64,
    pub accessed: i64,
    pub permissions: u32,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub is_hidden: bool,
    pub is_encrypted: bool,
    pub mime_type: Option<String>,
    pub file_hash: Option<String>,
    pub chunk_hashes: Vec<String>,
    pub metadata_hash: String,
    pub storage_class: Option<String>,
    pub encryption_key_id: Option<String>,
    pub version: Option<String>,
    pub tags: Vec<String>,
    pub custom_metadata: std::collections::HashMap<String, String>,
}

impl FileMetadata {
    pub fn new(path: PathBuf) -> Self {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        Self {
            path,
            size: 0,
            modified: now,
            created: now,
            accessed: now,
            permissions: 0o644,
            is_dir: false,
            is_symlink: false,
            is_hidden: false,
            is_encrypted: false,
            mime_type: None,
            file_hash: None,
            chunk_hashes: Vec::new(),
            metadata_hash: String::new(),
            storage_class: None,
            encryption_key_id: None,
            version: None,
            tags: Vec::new(),
            custom_metadata: std::collections::HashMap::new(),
        }
    }

    pub fn from_path(path: &Path) -> Result<Self> {
        let metadata = std::fs::metadata(path)?;

        let mut file_metadata = Self::new(path.to_path_buf());

        file_metadata.size = metadata.len();
        file_metadata.is_dir = metadata.is_dir();
        file_metadata.is_symlink = metadata.file_type().is_symlink();

        if let Ok(modified) = metadata.modified() {
            file_metadata.modified = modified
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
        }

        if let Ok(created) = metadata.created() {
            file_metadata.created = created
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
        }

        if let Ok(accessed) = metadata.accessed() {
            file_metadata.accessed = accessed
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
        }

        // 检测隐藏文件（Unix 系统以 . 开头）
        if let Some(file_name) = path.file_name() {
            if file_name.to_string_lossy().starts_with('.') {
                file_metadata.is_hidden = true;
            }
        }

        // 检测 MIME 类型
        if let Some(extension) = path.extension() {
            file_metadata.mime_type = Some(detect_mime_type(extension));
        }

        Ok(file_metadata)
    }

    pub fn calculate_hash(&mut self, _algorithm: ChecksumType) -> Result<()> {
        use sha2::{Digest, Sha256};
        use std::fs::File;
        use std::io::Read;

        if self.is_dir {
            self.file_hash = Some(String::new());
            return Ok(());
        }

        let mut file = File::open(&self.path)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0; 8192];

        loop {
            let bytes_read = file.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }

        let hash = format!("{:x}", hasher.finalize());
        self.file_hash = Some(hash);

        Ok(())
    }

    pub fn update_metadata_hash(&mut self) {
        let mut hasher = Sha256::new();
        hasher.update(self.path.to_string_lossy().as_bytes());
        hasher.update(&self.size.to_be_bytes());
        hasher.update(&self.modified.to_be_bytes());
        hasher.update(&self.permissions.to_be_bytes());

        if let Some(hash) = &self.file_hash {
            hasher.update(hash.as_bytes());
        }

        self.metadata_hash = format!("{:x}", hasher.finalize());
    }
}

/// 变化详情
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChangeDetails {
    /// 旧路径（用于重命名/移动）
    pub old_path: Option<String>,
    /// 内容变化类型
    pub content_change: ContentChangeType,
    /// 元数据变化
    pub metadata_changed: bool,
    /// 权限变化
    pub permissions_changed: bool,
    /// 时间戳变化
    pub timestamps_changed: bool,
    /// 重命名检测置信度（0-100）
    pub rename_confidence: u8,
    /// 变化百分比（0-100）
    pub change_percentage: u8,
    /// 变化的字节范围
    pub changed_ranges: Vec<(u64, u64)>,
    /// 新增行数（文本文件）
    pub lines_added: Option<usize>,
    /// 删除行数（文本文件）
    pub lines_removed: Option<usize>,
    /// 二进制变化检测
    pub binary_changes: Option<BinaryChanges>,
}

/// 内容变化类型
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ContentChangeType {
    #[default]
    Unknown,
    /// 新增文件
    Added,
    /// 删除文件
    Removed,
    /// 完全重写
    Rewritten,
    /// 部分修改
    Partial,
    /// 仅元数据变化
    MetadataOnly,
    /// 移动/重命名
    Moved,
    /// 内容未变
    Unchanged,
}

/// 二进制文件变化详情
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryChanges {
    /// 不同字节数
    pub different_bytes: u64,
    /// 相同字节数
    pub same_bytes: u64,
    /// 变化模式（连续变化区域等）
    pub change_patterns: Vec<ChangePattern>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangePattern {
    pub start: u64,
    pub end: u64,
    pub pattern_type: PatternType,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PatternType {
    Inserted,
    Deleted,
    Modified,
    Moved,
}

/// 校验和类型
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChecksumType {
    Md5,
    Sha1,
    Sha256,
    Sha512,
    Blake3,
    Crc32,
    Crc64,
}

impl ChecksumType {
    pub fn hash_size(&self) -> usize {
        match self {
            Self::Md5 => 16,
            Self::Sha1 => 20,
            Self::Sha256 => 32,
            Self::Sha512 => 64,
            Self::Blake3 => 32,
            Self::Crc32 => 4,
            Self::Crc64 => 8,
        }
    }

    pub fn recommended() -> Self {
        Self::Sha256
    }
}

/// 差异结果集合
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffResult {
    /// 所有文件差异
    pub files: Vec<FileDiff>,
    /// 总文件数
    pub total_files: usize,
    /// 需要传输的文件数
    pub files_to_transfer: usize,
    /// 需要删除的文件数
    pub files_to_delete: usize,
    /// 冲突文件数
    pub conflicts: usize,
    /// 总传输大小（字节）
    pub total_transfer_size: u64,
    /// 总删除大小（字节）
    pub total_delete_size: u64,
    /// 预计传输时间（毫秒）
    pub estimated_duration_ms: u64,
    /// 差异计算时间
    pub calculation_time_ms: u64,
    /// 来源统计
    pub source_stats: DiffStats,
    /// 目标统计
    pub target_stats: DiffStats,
    /// 操作统计
    pub action_stats: std::collections::HashMap<DiffAction, usize>,
}

impl DiffResult {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            total_files: 0,
            files_to_transfer: 0,
            files_to_delete: 0,
            conflicts: 0,
            total_transfer_size: 0,
            total_delete_size: 0,
            estimated_duration_ms: 0,
            calculation_time_ms: 0,
            source_stats: DiffStats::new(),
            target_stats: DiffStats::new(),
            action_stats: std::collections::HashMap::new(),
        }
    }

    pub fn add_file(&mut self, diff: FileDiff) {
        // 更新操作统计
        *self.action_stats.entry(diff.action).or_insert(0) += 1;

        // 更新大小统计
        match diff.action {
            DiffAction::Upload | DiffAction::Download | DiffAction::Update => {
                self.files_to_transfer += 1;
                self.total_transfer_size += diff.transfer_size();
            }
            DiffAction::Delete => {
                self.files_to_delete += 1;
                self.total_delete_size += diff.total_size();
            }
            DiffAction::Conflict => {
                self.conflicts += 1;
            }
            _ => {}
        }

        // 更新源和目标统计
        if let Some(source) = &diff.source_info {
            self.source_stats.add_file(source);
        }
        if let Some(target) = &diff.target_info {
            self.target_stats.add_file(target);
        }

        self.files.push(diff);
        self.total_files += 1;
    }

    pub fn sort_by_priority(&mut self) {
        self.files.sort_by(|a, b| b.priority.cmp(&a.priority));
    }

    pub fn filter_by_action(&self, action: DiffAction) -> Vec<&FileDiff> {
        self.files
            .iter()
            .filter(|diff| diff.action == action)
            .collect()
    }

    pub fn filter_by_tag(&self, tag: &str) -> Vec<&FileDiff> {
        self.files
            .iter()
            .filter(|diff| diff.tags.contains(&tag.to_string()))
            .collect()
    }

    pub fn find_by_path(&self, path: &str) -> Option<&FileDiff> {
        self.files.iter().find(|diff| diff.path == path)
    }

    pub fn has_conflicts(&self) -> bool {
        self.conflicts > 0
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn summary(&self) -> String {
        format!(
            "文件总数: {}, 需要传输: {} ({})，需要删除: {}，冲突: {}",
            self.total_files,
            self.files_to_transfer,
            format_bytes(self.total_transfer_size),
            self.files_to_delete,
            self.conflicts
        )
    }

    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| crate::error::SyncError::Serialization(e.into()))
    }

    pub fn to_csv(&self) -> Result<String> {
        let mut wtr = csv::Writer::from_writer(Vec::new());

        for diff in &self.files {
            wtr.serialize(CsvDiff {
                path: &diff.path,
                action: diff.action.as_str(),
                size: diff.total_size(),
                priority: diff.priority,
                estimated_duration_ms: diff.estimated_duration_ms,
                retry_count: diff.retry_count,
                requires_encryption: diff.requires_encryption,
                requires_chunking: diff.requires_chunking,
                tags: diff.tags.join(","),
            })
            .map_err(|_e| SyncError::Unsupported("转换异常".into()))?;
        }

        let data = String::from_utf8(
            wtr.into_inner()
                .map_err(|_e| SyncError::Unsupported("转换异常".into()))?,
        )
        .map_err(|e| SyncError::Validation(e.to_string()))?;

        Ok(data)
    }
}

/// 差异统计
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffStats {
    pub total_files: usize,
    pub total_dirs: usize,
    pub total_size: u64,
    pub largest_file: u64,
    pub smallest_file: u64,
    pub average_file_size: f64,
    pub file_types: std::collections::HashMap<String, usize>,
    pub oldest_file: Option<String>,
    pub newest_file: Option<String>,
}

impl DiffStats {
    pub fn new() -> Self {
        Self {
            total_files: 0,
            total_dirs: 0,
            total_size: 0,
            largest_file: 0,
            smallest_file: u64::MAX,
            average_file_size: 0.0,
            file_types: std::collections::HashMap::new(),
            oldest_file: None,
            newest_file: None,
        }
    }

    pub fn add_file(&mut self, metadata: &FileMetadata) {
        if metadata.is_dir {
            self.total_dirs += 1;
        } else {
            self.total_files += 1;
            self.total_size += metadata.size;

            // 更新最大/最小文件
            if metadata.size > self.largest_file {
                self.largest_file = metadata.size;
            }
            if metadata.size < self.smallest_file {
                self.smallest_file = metadata.size;
            }

            // 更新文件类型统计
            if let Some(mime_type) = &metadata.mime_type {
                *self.file_types.entry(mime_type.clone()).or_insert(0) += 1;
            }
        }
    }

    pub fn finalize(&mut self) {
        if self.total_files > 0 {
            self.average_file_size = self.total_size as f64 / self.total_files as f64;
        } else {
            self.smallest_file = 0;
        }
    }

    pub fn human_readable(&self) -> String {
        format!(
            "文件: {}, 目录: {}, 大小: {}",
            self.total_files,
            self.total_dirs,
            format_bytes(self.total_size)
        )
    }
}

/// CSV格式的差异记录
#[derive(Debug, Serialize)]
struct CsvDiff<'a> {
    path: &'a str,
    action: &'static str,
    size: u64,
    priority: u8,
    estimated_duration_ms: u64,
    retry_count: u32,
    requires_encryption: bool,
    requires_chunking: bool,
    tags: String,
}

/// 差异检测器
pub struct DiffDetector {
    options: DiffOptions,
    cache: std::collections::HashMap<String, FileMetadata>,
}

impl DiffDetector {
    pub fn new(options: DiffOptions) -> Self {
        Self {
            options,
            cache: std::collections::HashMap::new(),
        }
    }

    pub async fn detect_changes(
        &mut self,
        source_files: &[FileMetadata],
        target_files: &[FileMetadata],
    ) -> Result<DiffResult> {
        let start_time = std::time::Instant::now();
        let mut result = DiffResult::new();

        // 将目标文件转换为哈希映射以便快速查找
        let mut target_map = std::collections::HashMap::new();
        for file in target_files {
            target_map.insert(file.path.to_string_lossy().to_string(), file.clone());
        }

        // 检查源文件的差异
        for source_file in source_files {
            let path = source_file.path.to_string_lossy().to_string();

            if let Some(target_file) = target_map.remove(&path) {
                // 文件在两端都存在
                if self.is_file_changed(&source_file, &target_file) {
                    let diff = self.create_file_diff(&source_file, Some(&target_file));
                    result.add_file(diff);
                } else {
                    let diff = FileDiff::unchanged(path, source_file.clone(), target_file);
                    result.add_file(diff);
                }
            } else {
                // 文件只存在于源端（需要上传）
                let diff = FileDiff::upload(path, source_file.clone(), None);
                result.add_file(diff);
            }
        }

        // 剩余的目标文件只存在于目标端（需要删除或下载）
        for (path, target_file) in target_map {
            let diff = FileDiff::delete(path, target_file);
            result.add_file(diff);
        }

        // 检测文件移动/重命名
        self.detect_moves(&mut result);

        // 检测冲突
        self.detect_conflicts(&mut result);

        // 更新缓存
        self.update_cache(source_files);

        // 计算统计信息
        result.source_stats.finalize();
        result.target_stats.finalize();
        result.calculation_time_ms = start_time.elapsed().as_millis() as u64;
        result.estimated_duration_ms = result
            .files
            .iter()
            .filter(|diff| diff.action.is_transfer())
            .map(|diff| diff.estimated_duration_ms)
            .sum();

        result.sort_by_priority();
        Ok(result)
    }

    fn is_file_changed(&self, source: &FileMetadata, target: &FileMetadata) -> bool {
        if self.options.compare_size && source.size != target.size {
            return true;
        }

        if self.options.compare_mtime && source.modified != target.modified {
            return true;
        }

        if self.options.compare_checksum {
            match (&source.file_hash, &target.file_hash) {
                (Some(src_hash), Some(dst_hash)) if src_hash != dst_hash => return true,
                _ => {}
            }
        }

        if source.permissions != target.permissions {
            return true;
        }

        false
    }

    fn create_file_diff(&self, source: &FileMetadata, target: Option<&FileMetadata>) -> FileDiff {
        let path = source.path.to_string_lossy().to_string();

        match target {
            Some(target) => {
                let mut diff = FileDiff::update(path, source.clone(), target.clone());

                // 分析变化详情
                self.analyze_changes(&mut diff);
                diff
            }
            None => FileDiff::upload(path, source.clone(), None),
        }
    }

    fn analyze_changes(&self, diff: &mut FileDiff) {
        if let (Some(source), Some(target)) = (&diff.source_info, &diff.target_info) {
            let mut details = ChangeDetails::default();

            // 检查大小变化
            if source.size != target.size {
                details.content_change = ContentChangeType::Partial;
                details.change_percentage = if source.size > 0 {
                    ((source.size.abs_diff(target.size) * 100) / source.size) as u8
                } else {
                    100
                };
            }

            // 检查时间戳变化
            if source.modified != target.modified {
                details.timestamps_changed = true;
            }

            // 检查权限变化
            if source.permissions != target.permissions {
                details.permissions_changed = true;
            }

            diff.change_details = details;
        }
    }

    fn detect_moves(&self, result: &mut DiffResult) {
        // 实现文件移动检测算法
        // 基于文件大小、修改时间和内容相似度
        let mut potential_moves = Vec::new();

        for (i, diff_i) in result.files.iter().enumerate() {
            if diff_i.action == DiffAction::Delete {
                for (j, diff_j) in result.files.iter().enumerate() {
                    if diff_j.action == DiffAction::Upload {
                        if let (Some(src), Some(dst)) = (&diff_i.target_info, &diff_j.source_info) {
                            let similarity = self.calculate_file_similarity(src, dst);
                            if similarity > 0.8 {
                                potential_moves.push((i, j, similarity));
                            }
                        }
                    }
                }
            }
        }

        // 处理检测到的移动
        for (delete_idx, upload_idx, _similarity) in potential_moves {
            // 更新文件差异为移动操作
            let delete_path = result.files[delete_idx].path.clone();
            let upload_path = result.files[upload_idx].path.clone();

            if let (Some(source), Some(target)) = (
                result.files[upload_idx].source_info.clone(),
                result.files[delete_idx].target_info.clone(),
            ) {
                let move_diff = FileDiff::move_file(delete_path, upload_path, source, target);

                // 替换原来的差异
                result.files[delete_idx] = move_diff.clone();
                result.files[upload_idx] = move_diff;
            }
        }
    }

    fn detect_conflicts(&self, result: &mut DiffResult) {
        let mut path_map: std::collections::HashMap<String, Vec<usize>> =
            std::collections::HashMap::new();
        for (idx, diff) in result.files.iter().enumerate() {
            path_map.entry(diff.path.clone()).or_default().push(idx);
        }
        for indices in path_map.values() {
            if indices.len() > 1 {
                let has_upload = indices
                    .iter()
                    .any(|&i| result.files[i].action == DiffAction::Upload);
                let has_delete = indices
                    .iter()
                    .any(|&i| result.files[i].action == DiffAction::Delete);
                let has_update = indices
                    .iter()
                    .any(|&i| result.files[i].action == DiffAction::Update);
                if (has_upload && has_delete) || (has_upload && has_update) {
                    for &i in indices {
                        if let (Some(source), Some(target)) =
                            (&result.files[i].source_info, &result.files[i].target_info)
                        {
                            result.files[i] = FileDiff::conflict(
                                result.files[i].path.clone(),
                                source.clone(),
                                target.clone(),
                            );
                        }
                    }
                }
            }
        }
    }

    fn calculate_file_similarity(&self, file1: &FileMetadata, file2: &FileMetadata) -> f64 {
        let mut similarity = 0.0;

        // 大小相似度（权重40%）
        if file1.size == file2.size {
            similarity += 0.4;
        } else if file1.size > 0 && file2.size > 0 {
            let min_size = file1.size.min(file2.size) as f64;
            let max_size = file1.size.max(file2.size) as f64;
            similarity += 0.4 * (min_size / max_size);
        }

        // 修改时间相似度（权重30%）
        let time_diff = (file1.modified - file2.modified).abs();
        if time_diff < 60 {
            similarity += 0.3; // 时间差小于1分钟
        } else if time_diff < 3600 {
            similarity += 0.2; // 时间差小于1小时
        } else if time_diff < 86400 {
            similarity += 0.1; // 时间差小于1天
        }

        // 文件类型相似度（权重30%）
        if let (Some(mime1), Some(mime2)) = (&file1.mime_type, &file2.mime_type) {
            if mime1 == mime2 {
                similarity += 0.3;
            } else if mime1.split('/').next() == mime2.split('/').next() {
                similarity += 0.15; // 相同主类型
            }
        }

        similarity
    }

    fn update_cache(&mut self, files: &[FileMetadata]) {
        for file in files {
            self.cache
                .insert(file.path.to_string_lossy().to_string(), file.clone());
        }
    }
}

/// 差异检测选项
#[derive(Debug, Clone)]
pub struct DiffOptions {
    /// 比较文件大小
    pub compare_size: bool,
    /// 比较修改时间
    pub compare_mtime: bool,
    /// 比较文件校验和
    pub compare_checksum: bool,
    /// 忽略模式列表
    pub ignore_patterns: Vec<String>,
    /// 最大检测深度
    pub max_depth: Option<usize>,
    /// 是否跟随符号链接
    pub follow_symlinks: bool,
    /// 是否检测文件移动
    pub detect_moves: bool,
    /// 相似度阈值（用于移动检测）
    pub similarity_threshold: f64,
    /// 是否检测冲突
    pub detect_conflicts: bool,
    /// 是否包含隐藏文件
    pub include_hidden: bool,
    /// 文件大小阈值（大文件处理）
    pub large_file_threshold: u64,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self {
            compare_size: true,
            compare_mtime: true,
            compare_checksum: false, // 默认关闭，因为计算哈希较慢
            ignore_patterns: vec![
                ".*".to_string(),
                "*/.*".to_string(),
                "*.tmp".to_string(),
                "*.temp".to_string(),
            ],
            max_depth: None,
            follow_symlinks: false,
            detect_moves: true,
            similarity_threshold: 0.7,
            detect_conflicts: true,
            include_hidden: false,
            large_file_threshold: 1024 * 1024 * 100, // 100MB
        }
    }
}

fn detect_mime_type(extension: &std::ffi::OsStr) -> String {
    let ext = extension.to_string_lossy().to_lowercase();

    match ext.as_str() {
        "txt" => "text/plain",
        "json" => "application/json",
        "xml" => "application/xml",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" => "application/javascript",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "tar" => "application/x-tar",
        "gz" => "application/gzip",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "avi" => "video/x-msvideo",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "md" => "text/markdown",
        "yml" | "yaml" => "text/yaml",
        "toml" => "application/toml",
        "rs" => "text/x-rust",
        "go" => "text/x-go",
        "py" => "text/x-python",
        "java" => "text/x-java",
        "c" => "text/x-c",
        "cpp" | "cc" => "text/x-c++",
        "h" | "hpp" => "text/x-c++",
        _ => "application/octet-stream",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_diff_result_add_file_and_summary() {
        let mut result = DiffResult::new();
        let file = FileDiff::new(
            "a.txt".to_string(),
            DiffAction::Upload,
            Some(FileMetadata::new(PathBuf::from("a.txt"))),
            None,
        );
        result.add_file(file);
        assert_eq!(result.total_files, 1);
        let s = result.summary();
        assert!(s.contains("文件总数"));
    }
}
