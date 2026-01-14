use super::FileMetadata;
use crate::config::SyncTask;
use crate::error::SyncError;
use crate::report::SyncReport;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PluginHook {
    /// 同步前钩子
    PreSync {
        task_id: String,
        priority: HookPriority,
    },
    /// 同步后钩子
    PostSync {
        task_id: String,
        priority: HookPriority,
    },
    /// 文件上传前钩子
    PreFileUpload {
        task_id: String,
        priority: HookPriority,
    },
    /// 文件上传后钩子
    PostFileUpload {
        task_id: String,
        priority: HookPriority,
    },
    /// 错误处理钩子
    OnError {
        priority: HookPriority,
    },
    /// 文件过滤器钩子
    FileFilter {
        priority: HookPriority,
    },
    /// 文件名转换钩子
    FilenameTransform {
        priority: HookPriority,
    },
    /// 加密前钩子
    PreEncryption {
        priority: HookPriority,
    },
    /// 解密后钩子
    PostDecryption {
        priority: HookPriority,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum HookPriority {
    /// 最高优先级（最先执行）
    Highest = 100,
    /// 高优先级
    High = 80,
    /// 普通优先级（默认）
    Normal = 50,
    /// 低优先级
    Low = 20,
    /// 最低优先级（最后执行）
    Lowest = 0,
}

impl PluginHook {
    pub fn name(&self) -> &'static str {
        match self {
            Self::PreSync { .. } => "pre_sync",
            Self::PostSync { .. } => "post_sync",
            Self::PreFileUpload { .. } => "pre_file_upload",
            Self::PostFileUpload { .. } => "post_file_upload",
            Self::OnError { .. } => "on_error",
            Self::FileFilter { .. } => "file_filter",
            Self::FilenameTransform { .. } => "filename_transform",
            Self::PreEncryption { .. } => "pre_encryption",
            Self::PostDecryption { .. } => "post_decryption",
        }
    }

    pub fn priority(&self) -> HookPriority {
        match self {
            Self::PreSync { priority, .. } => priority.clone(),
            Self::PostSync { priority, .. } => priority.clone(),
            Self::PreFileUpload { priority, .. } => priority.clone(),
            Self::PostFileUpload { priority, .. } => priority.clone(),
            Self::OnError { priority, .. } => priority.clone(),
            Self::FileFilter { priority, .. } => priority.clone(),
            Self::FilenameTransform { priority, .. } => priority.clone(),
            Self::PreEncryption { priority, .. } => priority.clone(),
            Self::PostDecryption { priority, .. } => priority.clone(),
        }
    }

    pub fn set_priority(&mut self, priority: HookPriority) {
        match self {
            Self::PreSync { priority: p, .. } => *p = priority,
            Self::PostSync { priority: p, .. } => *p = priority,
            Self::PreFileUpload { priority: p, .. } => *p = priority,
            Self::PostFileUpload { priority: p, .. } => *p = priority,
            Self::OnError { priority: p, .. } => *p = priority,
            Self::FileFilter { priority: p, .. } => *p = priority,
            Self::FilenameTransform { priority: p, .. } => *p = priority,
            Self::PreEncryption { priority: p, .. } => *p = priority,
            Self::PostDecryption { priority: p, .. } => *p = priority,
        }
    }
}

#[derive(Debug)]
pub struct HookContext {
    pub task: Option<SyncTask>,
    pub report: Option<SyncReport>,
    pub file: Option<FileMetadata>,
    pub error: Option<SyncError>,
    pub custom_data: std::collections::HashMap<String, serde_json::Value>,
}

impl HookContext {
    pub fn new() -> Self {
        Self {
            task: None,
            report: None,
            file: None,
            error: None,
            custom_data: std::collections::HashMap::new(),
        }
    }

    pub fn with_task(mut self, task: SyncTask) -> Self {
        self.task = Some(task);
        self
    }

    pub fn with_report(mut self, report: SyncReport) -> Self {
        self.report = Some(report);
        self
    }

    pub fn with_file(mut self, file: FileMetadata) -> Self {
        self.file = Some(file);
        self
    }

    pub fn with_error(mut self, error: SyncError) -> Self {
        self.error = Some(error);
        self
    }

    pub fn set_custom_data(&mut self, key: String, value: serde_json::Value) {
        self.custom_data.insert(key, value);
    }

    pub fn get_custom_data(&self, key: &str) -> Option<&serde_json::Value> {
        self.custom_data.get(key)
    }
}

#[async_trait]
pub trait HookHandler: Send + Sync {
    /// 处理钩子
    async fn handle_hook(&self, hook: PluginHook, context: &mut HookContext) -> Result<(), SyncError>;

    /// 钩子是否支持特定类型
    fn supports_hook(&self, hook: &PluginHook) -> bool;

    /// 获取钩子优先级
    fn get_priority(&self, hook: &PluginHook) -> HookPriority;
}