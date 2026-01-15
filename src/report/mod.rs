use crate::error::SyncError;
use crate::utils::format_bytes;
use crate::sync::diff::{DiffAction, FileDiff};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct SyncReport {
    pub task_id: String,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub status: SyncStatus,
    pub statistics: SyncStatistics,
    pub files: Vec<FileSyncResult>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub duration_seconds: i64,
}

impl SyncReport {
    pub(crate) fn add_conflict(&self, _diff_path: &String) {
        // 简化：统计冲突数量
    }

    pub(crate) fn add_success(&self, _diff_path: &String, diff_size: i64) {
        // 简化：统计计数与传输字节
    }
}


impl SyncReport {
    pub fn new() -> SyncReport {
        SyncReport {
            task_id: "".into(),
            start_time: Default::default(),
            end_time: None,
            status: SyncStatus::Success,
            statistics: SyncStatistics::default(),
            files: vec![],
            errors: vec![],
            warnings: vec![],
            duration_seconds: 0,
        }
    }
    pub fn generate_html(&self) -> String {
        format!(r#"
<!DOCTYPE html>
<html>
<head>
    <title>Sync Report - {}</title>
    <style>
        body {{ font-family: Arial, sans-serif; margin: 40px; }}
        .summary {{ background: #f5f5f5; padding: 20px; border-radius: 5px; }}
        .stats {{ display: grid; grid-template-columns: repeat(3, 1fr); gap: 20px; }}
        .stat-box {{ background: white; padding: 15px; border-radius: 5px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }}
        .success {{ color: green; }}
        .error {{ color: red; }}
        .warning {{ color: orange; }}
        table {{ width: 100%; border-collapse: collapse; margin-top: 20px; }}
        th, td {{ border: 1px solid #ddd; padding: 8px; text-align: left; }}
        th {{ background-color: #4CAF50; color: white; }}
        tr:nth-child(even) {{ background-color: #f2f2f2; }}
    </style>
</head>
<body>
    <h1>Sync Report: {}</h1>
    <div class="summary">
        <h2>Summary</h2>
        <div class="stats">
            <div class="stat-box">
                <h3>Files Synced</h3>
                <p class="success">{}</p>
            </div>
            <div class="stat-box">
                <h3>Transfer Speed</h3>
                <p>{:.2} MB/s</p>
            </div>
            <div class="stat-box">
                <h3>Duration</h3>
                <p>{:.1} seconds</p>
            </div>
        </div>
    </div>

    <h2>File Details</h2>
    <table>
        <thead>
            <tr>
                <th>File</th>
                <th>Size</th>
                <th>Status</th>
                <th>Time</th>
            </tr>
        </thead>
        <tbody>
            {}
        </tbody>
    </table>

    <h2>Errors ({})</h2>
    <ul>
        {}
    </ul>
</body>
</html>
        "#,
                self.task_id,
                self.task_id,
                self.statistics.files_synced,
                self.statistics.transfer_rate / 1024.0 / 1024.0,
                self.statistics.duration_seconds,
                self.generate_file_rows(),
                self.errors.len(),
                self.generate_error_list()
        )
    }

    pub fn summary(&self) -> String {
        self.statistics.summary()
    }

    fn generate_file_rows(&self) -> String {
        let mut rows = String::new();
        for f in &self.files {
            rows.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                f.path,
                f.human_readable_size(),
                f.status.as_str(),
                f.duration_ms.unwrap_or(0)
            ));
        }
        rows
    }

    fn generate_error_list(&self) -> String {
        let mut list = String::new();
        for e in &self.errors {
            list.push_str(&format!("<li>{}</li>", e));
        }
        list
    }

    pub fn generate_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    pub fn save(&self) {}

}


/// 同步状态枚举
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SyncStatus {
    /// 等待中（尚未开始）
    Pending,
    /// 运行中
    Running,
    /// 成功完成
    Success,
    /// 部分成功（有部分文件失败）
    PartialSuccess,
    /// 失败
    Failed,
    /// 已取消
    Cancelled,
    /// 正在重试
    Retrying,
    /// 暂停
    Paused,
    /// 验证中
    Verifying,
    /// 修复中
    Repairing,
}

impl SyncStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "待开始",
            Self::Running => "运行中",
            Self::Success => "成功",
            Self::PartialSuccess => "部分成功",
            Self::Failed => "失败",
            Self::Cancelled => "已取消",
            Self::Retrying => "重试中",
            Self::Paused => "已暂停",
            Self::Verifying => "验证中",
            Self::Repairing => "修复中",
        }
    }

    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Pending => "⏳",
            Self::Running => "🔄",
            Self::Success => "✅",
            Self::PartialSuccess => "⚠️",
            Self::Failed => "❌",
            Self::Cancelled => "🚫",
            Self::Retrying => "🔄",
            Self::Paused => "⏸️",
            Self::Verifying => "🔍",
            Self::Repairing => "🔧",
        }
    }

    pub fn is_completed(&self) -> bool {
        matches!(self,
            Self::Success | Self::PartialSuccess |
            Self::Failed | Self::Cancelled
        )
    }

    pub fn is_successful(&self) -> bool {
        matches!(self, Self::Success | Self::PartialSuccess)
    }

    pub fn can_retry(&self) -> bool {
        matches!(self, Self::Failed | Self::PartialSuccess)
    }

    pub fn is_active(&self) -> bool {
        matches!(self, Self::Running | Self::Retrying | Self::Verifying | Self::Repairing)
    }
}

/// 文件同步结果
#[derive(Debug, Serialize, Deserialize)]
pub struct FileSyncResult {
    /// 文件路径
    pub path: String,
    /// 文件状态
    pub status: FileSyncStatus,
    /// 文件大小（字节）
    pub size: u64,
    /// 传输大小（实际传输的字节数，可能包括加密开销）
    pub transferred_size: u64,
    /// 源文件哈希
    pub source_hash: Option<String>,
    /// 目标文件哈希
    pub target_hash: Option<String>,
    /// 文件同步操作
    pub operation: FileOperation,
    /// 开始时间
    pub start_time: Option<DateTime<Utc>>,
    /// 结束时间
    pub end_time: Option<DateTime<Utc>>,
    /// 传输耗时（毫秒）
    pub duration_ms: Option<u64>,
    /// 传输速度（字节/秒）
    pub transfer_speed: Option<f64>,
    /// 重试次数
    pub retry_count: u32,
    /// 是否加密传输
    pub encrypted: bool,
    /// 是否分块传输
    pub chunked: bool,
    /// 分块数量
    pub chunk_count: Option<usize>,
    /// 校验和验证结果
    pub checksum_verified: Option<bool>,
    /// 错误信息（如果有）
    pub error: Option<String>,
    /// 警告信息
    pub warnings: Vec<String>,
    /// 自定义元数据
    pub metadata: serde_json::Value,
    /// 操作ID（用于去重和跟踪）
    pub operation_id: String,
    /// 源文件路径（可能不同）
    pub source_path: Option<String>,
    /// 目标文件路径
    pub target_path: String,
    /// 文件类型
    pub file_type: FileType,
    /// 文件权限
    pub permissions: Option<u32>,
    /// 文件修改时间
    pub modified_time: Option<DateTime<Utc>>,
}

impl FileSyncResult {
    pub fn new(path: String, operation: FileOperation) -> Self {
        Self {
            path: path.clone(),
            status: FileSyncStatus::Pending,
            size: 0,
            transferred_size: 0,
            source_hash: None,
            target_hash: None,
            operation,
            start_time: None,
            end_time: None,
            duration_ms: None,
            transfer_speed: None,
            retry_count: 0,
            encrypted: false,
            chunked: false,
            chunk_count: None,
            checksum_verified: None,
            error: None,
            warnings: Vec::new(),
            metadata: serde_json::Value::Null,
            operation_id: Self::generate_operation_id(&path),
            source_path: None,
            target_path: path,
            file_type: FileType::Unknown,
            permissions: None,
            modified_time: None,
        }
    }

    pub fn from_diff(diff: &FileDiff, operation: FileOperation) -> Self {
        let size = diff.source_info.as_ref().map(|f| f.size).unwrap_or(0);

        let mut result = Self::new(diff.path.clone(), operation);
        result.size = size;

        if let Some(source_info) = &diff.source_info {
            result.source_hash = source_info.file_hash.clone();
            result.file_type = FileType::from_metadata(source_info);
            result.modified_time = Some(DateTime::from_timestamp(
                source_info.modified, 0,
            ).unwrap_or_else(|| Utc::now()));
            result.permissions = Some(source_info.permissions);
        }

        if let Some(target_info) = &diff.target_info {
            result.target_hash = target_info.file_hash.clone();
        }

        result.encrypted = diff.requires_encryption;
        result.chunked = diff.requires_chunking;

        result
    }

    fn generate_operation_id(path: &str) -> String {
        use uuid::Uuid;
        format!("op_{}_{}",
                path.replace("/", "_").replace(".", "_"),
                Uuid::new_v4().simple()
        )
    }

    pub fn mark_started(&mut self) {
        self.start_time = Some(Utc::now());
        self.status = FileSyncStatus::Transferring;
    }

    pub fn mark_completed(&mut self, success: bool) {
        self.end_time = Some(Utc::now());
        self.status = if success {
            FileSyncStatus::Success
        } else {
            FileSyncStatus::Failed
        };

        // 计算耗时
        if let (Some(start), Some(end)) = (self.start_time, self.end_time) {
            let duration = end - start;
            self.duration_ms = Some(duration.num_milliseconds() as u64);

            // 计算传输速度
            if self.transferred_size > 0 && self.duration_ms.unwrap_or(0) > 0 {
                let duration_secs = self.duration_ms.unwrap() as f64 / 1000.0;
                self.transfer_speed = Some(self.transferred_size as f64 / duration_secs);
            }
        }
    }

    pub fn mark_retry(&mut self, error: Option<String>) {
        self.status = FileSyncStatus::Retrying;
        self.retry_count += 1;
        self.error = error;
        self.start_time = Some(Utc::now()); // 重置开始时间
    }

    pub fn mark_verifying(&mut self) {
        self.status = FileSyncStatus::Verifying;
    }

    pub fn mark_verified(&mut self, verified: bool) {
        self.checksum_verified = Some(verified);
        self.status = if verified {
            FileSyncStatus::Success
        } else {
            FileSyncStatus::Failed
        };
    }

    pub fn add_warning(&mut self, warning: String) {
        self.warnings.push(warning);
    }

    pub fn is_success(&self) -> bool {
        self.status == FileSyncStatus::Success
    }

    pub fn is_failed(&self) -> bool {
        self.status == FileSyncStatus::Failed
    }

    pub fn is_completed(&self) -> bool {
        self.status.is_completed()
    }

    pub fn duration(&self) -> Option<std::time::Duration> {
        self.duration_ms.map(|ms| std::time::Duration::from_millis(ms))
    }

    pub fn human_readable_size(&self) -> String {
        format_bytes(self.size)
    }

    pub fn human_readable_transferred(&self) -> String {
        format_bytes(self.transferred_size)
    }

    pub fn human_readable_speed(&self) -> Option<String> {
        self.transfer_speed.map(|speed| {
            if speed >= 1024.0 * 1024.0 {
                format!("{:.1} MB/s", speed / (1024.0 * 1024.0))
            } else if speed >= 1024.0 {
                format!("{:.1} KB/s", speed / 1024.0)
            } else {
                format!("{:.1} B/s", speed)
            }
        })
    }

    pub fn summary(&self) -> String {
        let status_emoji = self.status.emoji();
        let operation_str = self.operation.as_str();

        let mut summary = format!("{} {}: {}", status_emoji, operation_str, self.path);

        if let Some(duration) = self.duration() {
            let secs = duration.as_secs_f64();
            if secs > 0.0 {
                summary.push_str(&format!(" ({:.2}s)", secs));
            }
        }

        if let Some(speed) = self.human_readable_speed() {
            summary.push_str(&format!(" @ {}", speed));
        }

        summary
    }

    pub fn detailed_info(&self) -> String {
        let mut info = format!("文件: {}\n", self.path);
        info.push_str(&format!("状态: {} {}\n", self.status.emoji(), self.status.as_str()));
        info.push_str(&format!("操作: {}\n", self.operation.as_str()));
        info.push_str(&format!("大小: {}\n", self.human_readable_size()));

        if self.transferred_size > 0 {
            info.push_str(&format!("传输大小: {}\n", self.human_readable_transferred()));
        }

        if let (Some(start), Some(end)) = (self.start_time, self.end_time) {
            info.push_str(&format!("开始时间: {}\n", start.format("%Y-%m-%d %H:%M:%S")));
            info.push_str(&format!("结束时间: {}\n", end.format("%Y-%m-%d %H:%M:%S")));
        }

        if let Some(duration) = self.duration() {
            let secs = duration.as_secs_f64();
            info.push_str(&format!("耗时: {:.2} 秒\n", secs));
        }

        if let Some(speed) = self.human_readable_speed() {
            info.push_str(&format!("速度: {}\n", speed));
        }

        if self.encrypted {
            info.push_str("加密: 是\n");
        }

        if self.chunked {
            info.push_str(&format!("分块传输: 是 ({}块)\n",
                                   self.chunk_count.unwrap_or(0)));
        }

        if let Some(verified) = self.checksum_verified {
            info.push_str(&format!("校验和验证: {}\n",
                                   if verified { "通过" } else { "失败" }));
        }

        if self.retry_count > 0 {
            info.push_str(&format!("重试次数: {}\n", self.retry_count));
        }

        if let Some(error) = &self.error {
            info.push_str(&format!("错误: {}\n", error));
        }

        if !self.warnings.is_empty() {
            info.push_str(&format!("警告: {}\n", self.warnings.join("; ")));
        }

        info
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

/// 文件同步状态
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum FileSyncStatus {
    /// 等待中
    Pending,
    /// 准备中
    Preparing,
    /// 传输中
    Transferring,
    /// 验证中
    Verifying,
    /// 重试中
    Retrying,
    /// 成功
    Success,
    /// 失败
    Failed,
    /// 跳过
    Skipped,
    /// 冲突
    Conflict,
    /// 部分成功（如分块传输）
    PartialSuccess,
    /// 取消
    Cancelled,
    /// 等待用户确认
    WaitingForUser,
}

impl FileSyncStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "等待",
            Self::Preparing => "准备",
            Self::Transferring => "传输中",
            Self::Verifying => "验证中",
            Self::Retrying => "重试中",
            Self::Success => "成功",
            Self::Failed => "失败",
            Self::Skipped => "跳过",
            Self::Conflict => "冲突",
            Self::PartialSuccess => "部分成功",
            Self::Cancelled => "已取消",
            Self::WaitingForUser => "等待用户",
        }
    }

    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Pending => "⏳",
            Self::Preparing => "📝",
            Self::Transferring => "📤",
            Self::Verifying => "🔍",
            Self::Retrying => "🔄",
            Self::Success => "✅",
            Self::Failed => "❌",
            Self::Skipped => "⏭️",
            Self::Conflict => "⚠️",
            Self::PartialSuccess => "⚠️",
            Self::Cancelled => "🚫",
            Self::WaitingForUser => "🤔",
        }
    }

    pub fn is_completed(&self) -> bool {
        matches!(self,
            Self::Success | Self::Failed |
            Self::Skipped | Self::Cancelled |
            Self::Conflict
        )
    }

    pub fn is_successful(&self) -> bool {
        matches!(self, Self::Success | Self::PartialSuccess)
    }

    pub fn can_retry(&self) -> bool {
        matches!(self, Self::Failed)
    }
}

/// 文件操作类型
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum FileOperation {
    /// 上传文件
    Upload,
    /// 下载文件
    Download,
    /// 删除文件
    Delete,
    /// 移动文件
    Move,
    /// 复制文件
    Copy,
    /// 更新文件（修改内容）
    Update,
    /// 更新元数据
    UpdateMetadata,
    /// 验证文件
    Verify,
    /// 修复文件
    Repair,
    /// 加密文件
    Encrypt,
    /// 解密文件
    Decrypt,
    /// 压缩文件
    Compress,
    /// 解压文件
    Decompress,
}

impl FileOperation {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Upload => "上传",
            Self::Download => "下载",
            Self::Delete => "删除",
            Self::Move => "移动",
            Self::Copy => "复制",
            Self::Update => "更新",
            Self::UpdateMetadata => "更新元数据",
            Self::Verify => "验证",
            Self::Repair => "修复",
            Self::Encrypt => "加密",
            Self::Decrypt => "解密",
            Self::Compress => "压缩",
            Self::Decompress => "解压",
        }
    }

    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Upload => "📤",
            Self::Download => "📥",
            Self::Delete => "🗑️",
            Self::Move => "📦",
            Self::Copy => "📋",
            Self::Update => "🔄",
            Self::UpdateMetadata => "📊",
            Self::Verify => "🔍",
            Self::Repair => "🔧",
            Self::Encrypt => "🔒",
            Self::Decrypt => "🔓",
            Self::Compress => "🗜️",
            Self::Decompress => "🗜️",
        }
    }

    pub fn from_diff_action(action: DiffAction) -> Self {
        match action {
            DiffAction::Upload => FileOperation::Upload,
            DiffAction::Download => FileOperation::Download,
            DiffAction::Delete => FileOperation::Delete,
            DiffAction::Move => FileOperation::Move,
            DiffAction::Update => FileOperation::Update,
            DiffAction::Conflict => FileOperation::Verify, // 冲突文件需要验证
            DiffAction::Unchanged => FileOperation::Verify,
        }
    }
}

/// 文件类型枚举
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum FileType {
    /// 未知类型
    Unknown,
    /// 文本文件
    Text,
    /// 图片文件
    Image,
    /// 视频文件
    Video,
    /// 音频文件
    Audio,
    /// 文档文件
    Document,
    /// 压缩文件
    Archive,
    /// 可执行文件
    Executable,
    /// 配置文件
    Config,
    /// 日志文件
    Log,
    /// 数据库文件
    Database,
    /// 代码文件
    Code,
    /// 字体文件
    Font,
    /// 3D模型文件
    Model3D,
    /// 电子表格
    Spreadsheet,
    /// 演示文稿
    Presentation,
    /// PDF文档
    Pdf,
    /// 目录
    Directory,
    /// 符号链接
    Symlink,
    /// 设备文件
    Device,
}

impl FileType {
    pub fn from_extension(extension: &str) -> Self {
        let ext = extension.to_lowercase();

        match ext.as_str() {
            "txt" | "md" | "markdown" | "rst" | "log" => FileType::Text,
            "jpg" | "jpeg" | "png" | "gif" | "bmp" | "svg" | "webp" | "ico" => FileType::Image,
            "mp4" | "avi" | "mkv" | "mov" | "wmv" | "flv" | "webm" => FileType::Video,
            "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" => FileType::Audio,
            "pdf" => FileType::Pdf,
            "doc" | "docx" | "odt" | "rtf" => FileType::Document,
            "xls" | "xlsx" | "ods" | "csv" => FileType::Spreadsheet,
            "ppt" | "pptx" | "odp" => FileType::Presentation,
            "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" => FileType::Archive,
            "exe" | "bin" | "app" | "sh" | "bat" | "cmd" => FileType::Executable,
            "json" | "yaml" | "yml" | "toml" | "ini" | "cfg" | "conf" => FileType::Config,
            "db" | "sqlite" | "mdb" | "accdb" => FileType::Database,
            "rs" | "py" | "js" | "ts" | "java" | "cpp" | "c" | "h" | "go" | "php" | "rb" => FileType::Code,
            "ttf" | "otf" | "woff" | "woff2" => FileType::Font,
            "stl" | "obj" | "fbx" | "blend" => FileType::Model3D,
            _ => FileType::Unknown,
        }
    }

    pub fn from_path(path: &Path) -> Self {
        if path.is_dir() {
            return FileType::Directory;
        }

        if let Some(extension) = path.extension() {
            Self::from_extension(extension.to_string_lossy().as_ref())
        } else {
            FileType::Unknown
        }
    }

    pub fn from_metadata(metadata: &crate::sync::diff::FileMetadata) -> Self {
        if metadata.is_dir {
            FileType::Directory
        } else if metadata.is_symlink {
            FileType::Symlink
        } else if let Some(mime_type) = &metadata.mime_type {
            Self::from_mime_type(mime_type)
        } else if let Some(extension) = metadata.path.extension() {
            Self::from_extension(extension.to_string_lossy().as_ref())
        } else {
            FileType::Unknown
        }
    }

    pub fn from_mime_type(mime_type: &str) -> Self {
        match mime_type {
            t if t.starts_with("text/") => FileType::Text,
            t if t.starts_with("image/") => FileType::Image,
            t if t.starts_with("video/") => FileType::Video,
            t if t.starts_with("audio/") => FileType::Audio,
            t if t.starts_with("application/pdf") => FileType::Pdf,
            t if t.contains("document") => FileType::Document,
            t if t.contains("spreadsheet") => FileType::Spreadsheet,
            t if t.contains("presentation") => FileType::Presentation,
            t if t.contains("zip") || t.contains("compressed") => FileType::Archive,
            t if t.contains("executable") || t.contains("octet-stream") => FileType::Executable,
            t if t.contains("json") || t.contains("yaml") || t.contains("xml") => FileType::Config,
            t if t.contains("database") => FileType::Database,
            t if t.contains("font") => FileType::Font,
            _ => FileType::Unknown,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unknown => "未知",
            Self::Text => "文本",
            Self::Image => "图片",
            Self::Video => "视频",
            Self::Audio => "音频",
            Self::Document => "文档",
            Self::Archive => "压缩包",
            Self::Executable => "可执行文件",
            Self::Config => "配置文件",
            Self::Log => "日志",
            Self::Database => "数据库",
            Self::Code => "代码",
            Self::Font => "字体",
            Self::Model3D => "3D模型",
            Self::Spreadsheet => "电子表格",
            Self::Presentation => "演示文稿",
            Self::Pdf => "PDF",
            Self::Directory => "目录",
            Self::Symlink => "符号链接",
            Self::Device => "设备文件",
        }
    }

    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Unknown => "📄",
            Self::Text => "📄",
            Self::Image => "🖼️",
            Self::Video => "🎬",
            Self::Audio => "🎵",
            Self::Document => "📄",
            Self::Archive => "🗜️",
            Self::Executable => "⚙️",
            Self::Config => "⚙️",
            Self::Log => "📋",
            Self::Database => "🗄️",
            Self::Code => "💻",
            Self::Font => "🔤",
            Self::Model3D => "🧊",
            Self::Spreadsheet => "📊",
            Self::Presentation => "📽️",
            Self::Pdf => "📘",
            Self::Directory => "📁",
            Self::Symlink => "🔗",
            Self::Device => "💾",
        }
    }

    pub fn is_compressible(&self) -> bool {
        matches!(self,
            Self::Text | Self::Code | Self::Document |
            Self::Spreadsheet | Self::Presentation |
            Self::Config | Self::Log
        )
    }

    pub fn is_binary(&self) -> bool {
        !matches!(self,
            Self::Text | Self::Code | Self::Config |
            Self::Document | Self::Spreadsheet |
            Self::Presentation | Self::Log
        )
    }
}

/// 同步统计信息
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncStatistics {
    /// 总文件数
    pub total_files: usize,
    /// 成功同步的文件数
    pub files_synced: usize,
    /// 跳过的文件数
    pub files_skipped: usize,
    /// 失败的文件数
    pub files_failed: usize,
    /// 冲突的文件数
    pub conflicts: usize,
    /// 总字节数
    pub total_bytes: u64,
    /// 已传输的字节数
    pub transferred_bytes: u64,
    /// 平均传输速率（字节/秒）
    pub average_speed: f64,
    /// 最大传输速率（字节/秒）
    pub max_speed: f64,
    /// 最小传输速率（字节/秒）
    pub min_speed: f64,
    /// 总耗时（秒）
    pub duration_seconds: f64,
    /// 重试总次数
    pub total_retries: u32,
    /// 加密文件数
    pub encrypted_files: usize,
    /// 分块传输的文件数
    pub chunked_files: usize,
    /// 校验和验证的文件数
    pub verified_files: usize,
    /// 校验和失败的文件数
    pub verification_failed: usize,
    /// 按文件类型统计
    pub file_type_stats: HashMap<FileType, FileTypeStats>,
    /// 按操作类型统计
    pub operation_stats: HashMap<FileOperation, usize>,
    // 传输速率
    pub transfer_rate: f64,
}

impl SyncStatistics {
    pub fn new() -> Self {
        Self {
            total_files: 0,
            files_synced: 0,
            files_skipped: 0,
            files_failed: 0,
            conflicts: 0,
            total_bytes: 0,
            transferred_bytes: 0,
            average_speed: 0.0,
            max_speed: 0.0,
            min_speed: f64::MAX,
            duration_seconds: 0.0,
            total_retries: 0,
            encrypted_files: 0,
            chunked_files: 0,
            verified_files: 0,
            verification_failed: 0,
            file_type_stats: HashMap::new(),
            operation_stats: HashMap::new(),
            transfer_rate: 0.0,
        }
    }

    pub fn add_file_result(&mut self, result: &FileSyncResult) {
        self.total_files += 1;

        match result.status {
            FileSyncStatus::Success | FileSyncStatus::PartialSuccess => {
                self.files_synced += 1;
            }
            FileSyncStatus::Failed => {
                self.files_failed += 1;
            }
            FileSyncStatus::Skipped => {
                self.files_skipped += 1;
            }
            FileSyncStatus::Conflict => {
                self.conflicts += 1;
            }
            _ => {}
        }

        self.total_bytes += result.size;
        self.transferred_bytes += result.transferred_size;
        self.total_retries += result.retry_count;

        if result.encrypted {
            self.encrypted_files += 1;
        }

        if result.chunked {
            self.chunked_files += 1;
        }

        if let Some(verified) = result.checksum_verified {
            if verified {
                self.verified_files += 1;
            } else {
                self.verification_failed += 1;
            }
        }

        // 更新文件类型统计
        let stats = self.file_type_stats.entry(result.file_type)
            .or_insert_with(FileTypeStats::new);
        stats.add_file(result);

        // 更新操作类型统计
        *self.operation_stats.entry(result.operation)
            .or_insert(0) += 1;
    }

    pub fn update_speed_metrics(&mut self, speed: f64) {
        self.average_speed = (self.average_speed + speed) / 2.0;
        self.max_speed = self.max_speed.max(speed);
        self.min_speed = self.min_speed.min(speed);
    }

    pub fn finalize(&mut self, duration: f64) {
        self.duration_seconds = duration;

        if self.duration_seconds > 0.0 {
            self.average_speed = self.transferred_bytes as f64 / self.duration_seconds;
        }

        if self.min_speed == f64::MAX {
            self.min_speed = 0.0;
        }
    }

    pub fn success_rate(&self) -> f64 {
        if self.total_files == 0 {
            return 0.0;
        }
        (self.files_synced as f64 / self.total_files as f64) * 100.0
    }

    pub fn failure_rate(&self) -> f64 {
        if self.total_files == 0 {
            return 0.0;
        }
        (self.files_failed as f64 / self.total_files as f64) * 100.0
    }

    pub fn skip_rate(&self) -> f64 {
        if self.total_files == 0 {
            return 0.0;
        }
        (self.files_skipped as f64 / self.total_files as f64) * 100.0
    }

    pub fn verification_success_rate(&self) -> f64 {
        let total_verified = self.verified_files + self.verification_failed;
        if total_verified == 0 {
            return 0.0;
        }
        (self.verified_files as f64 / total_verified as f64) * 100.0
    }

    pub fn average_file_size(&self) -> f64 {
        if self.total_files == 0 {
            0.0
        } else {
            self.total_bytes as f64 / self.total_files as f64
        }
    }

    pub fn human_readable_total_bytes(&self) -> String {
        format_bytes(self.total_bytes)
    }

    pub fn human_readable_transferred_bytes(&self) -> String {
        format_bytes(self.transferred_bytes)
    }

    pub fn human_readable_average_speed(&self) -> String {
        if self.average_speed >= 1024.0 * 1024.0 {
            format!("{:.1} MB/s", self.average_speed / (1024.0 * 1024.0))
        } else if self.average_speed >= 1024.0 {
            format!("{:.1} KB/s", self.average_speed / 1024.0)
        } else {
            format!("{:.1} B/s", self.average_speed)
        }
    }

    pub fn summary(&self) -> String {
        let success_rate = self.success_rate();
        let verification_rate = self.verification_success_rate();

        format!(
            "文件: {}/{} ({:.1}% 成功率), 数据: {}/{}, 速度: {}, 耗时: {:.1}s",
            self.files_synced,
            self.total_files,
            success_rate,
            self.human_readable_transferred_bytes(),
            self.human_readable_total_bytes(),
            self.human_readable_average_speed(),
            self.duration_seconds
        )
    }

    pub fn detailed_report(&self) -> String {
        let mut report = String::new();

        report.push_str(&format!("📊 同步统计详情\n"));
        report.push_str(&format!("文件总数: {}\n", self.total_files));
        report.push_str(&format!("成功同步: {} ({:.1}%)\n", self.files_synced, self.success_rate()));
        report.push_str(&format!("同步失败: {} ({:.1}%)\n", self.files_failed, self.failure_rate()));
        report.push_str(&format!("跳过文件: {} ({:.1}%)\n", self.files_skipped, self.skip_rate()));
        report.push_str(&format!("冲突文件: {}\n", self.conflicts));
        report.push_str(&format!("重试次数: {}\n", self.total_retries));
        report.push_str(&format!("总数据量: {}\n", self.human_readable_total_bytes()));
        report.push_str(&format!("传输数据: {}\n", self.human_readable_transferred_bytes()));
        report.push_str(&format!("平均文件大小: {:.1} KB\n", self.average_file_size() / 1024.0));
        report.push_str(&format!("平均速度: {}\n", self.human_readable_average_speed()));
        report.push_str(&format!("最大速度: {:.1} MB/s\n", self.max_speed / (1024.0 * 1024.0)));
        report.push_str(&format!("最小速度: {:.1} KB/s\n", self.min_speed / 1024.0));
        report.push_str(&format!("总耗时: {:.1} 秒\n", self.duration_seconds));
        report.push_str(&format!("加密文件: {}\n", self.encrypted_files));
        report.push_str(&format!("分块传输: {}\n", self.chunked_files));
        report.push_str(&format!("校验和验证: {}/{} ({:.1}% 成功率)\n",
                                 self.verified_files,
                                 self.verified_files + self.verification_failed,
                                 self.verification_success_rate()));

        report.push_str("\n📁 文件类型统计:\n");
        let mut file_types: Vec<_> = self.file_type_stats.iter().collect();
        file_types.sort_by_key(|(_, stats)| std::cmp::Reverse(stats.count));

        for (file_type, stats) in file_types.iter().take(10) {
            let percentage = if self.total_files > 0 {
                (stats.count as f64 / self.total_files as f64) * 100.0
            } else {
                0.0
            };
            report.push_str(&format!("  {} {}: {} ({:.1}%) - {}\n",
                                     file_type.emoji(),
                                     file_type.as_str(),
                                     stats.count,
                                     percentage,
                                     format_bytes(stats.total_size)));
        }

        report.push_str("\n🔄 操作类型统计:\n");
        let mut operations: Vec<_> = self.operation_stats.iter().collect();
        operations.sort_by_key(|(_, count)| std::cmp::Reverse(*count));

        for (operation, count) in operations {
            let percentage = if self.total_files > 0 {
                (*count as f64 / self.total_files as f64) * 100.0
            } else {
                0.0
            };
            report.push_str(&format!("  {} {}: {} ({:.1}%)\n",
                                     operation.emoji(),
                                     operation.as_str(),
                                     count,
                                     percentage));
        }

        report
    }
}

/// 文件类型统计
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTypeStats {
    /// 文件数量
    pub count: usize,
    /// 总大小
    pub total_size: u64,
    /// 成功数量
    pub success_count: usize,
    /// 失败数量
    pub failure_count: usize,
    /// 平均传输速度
    pub average_speed: f64,
    /// 总传输时间（秒）
    pub total_duration: f64,
    /// 加密文件数量
    pub encrypted_count: usize,
}

impl FileTypeStats {
    pub fn new() -> Self {
        Self {
            count: 0,
            total_size: 0,
            success_count: 0,
            failure_count: 0,
            average_speed: 0.0,
            total_duration: 0.0,
            encrypted_count: 0,
        }
    }

    pub fn add_file(&mut self, result: &FileSyncResult) {
        self.count += 1;
        self.total_size += result.size;

        if result.is_success() {
            self.success_count += 1;
        } else if result.is_failed() {
            self.failure_count += 1;
        }

        if let Some(duration) = result.duration() {
            self.total_duration += duration.as_secs_f64();

            if duration.as_secs_f64() > 0.0 && result.transferred_size > 0 {
                let speed = result.transferred_size as f64 / duration.as_secs_f64();
                self.average_speed = (self.average_speed + speed) / 2.0;
            }
        }

        if result.encrypted {
            self.encrypted_count += 1;
        }
    }

    pub fn success_rate(&self) -> f64 {
        if self.count == 0 {
            return 0.0;
        }
        (self.success_count as f64 / self.count as f64) * 100.0
    }
}
