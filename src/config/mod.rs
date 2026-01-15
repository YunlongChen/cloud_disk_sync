mod validator;
mod utils;
mod migrator;

use crate::encryption::types::{EncryptionAlgorithm, IvMode};
use crate::error::ConfigError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::fs::create_dir_all;
use std::path::PathBuf;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProviderType {
    AliYunDrive,
    OneOneFive,
    Quark,
    WebDAV,
    SMB,
    Local,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountConfig {
    pub id: String,
    pub provider: ProviderType,
    pub name: String,
    pub credentials: HashMap<String, String>,
    pub rate_limit: Option<RateLimitConfig>,
    pub retry_policy: RetryPolicy,
}

impl AccountConfig {
    pub fn validate(&self) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub requests_per_minute: u32,
    pub max_concurrent: usize,
    pub chunk_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
    pub backoff_factor: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        // todo 默认重试策略缺失
        RetryPolicy {
            max_retries: 0,
            initial_delay_ms: 0,
            max_delay_ms: 0,
            backoff_factor: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncTask {
    pub id: String,
    pub name: String,
    pub source_account: String,
    pub source_path: String,
    pub target_account: String,
    pub target_path: String,
    pub schedule: Option<Schedule>,
    pub filters: Vec<FilterRule>,
    pub encryption: Option<EncryptionConfig>,
    pub diff_mode: DiffMode,
    pub preserve_metadata: bool,
    pub verify_integrity: bool,
}

impl SyncTask {
    pub fn validate(&self) -> Result<(), Box<dyn Error>> {
        // todo 需要对同步任务进行校验
        info!("SyncTask::validate()");
        todo!()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Schedule {
    Cron(String),
    Interval { seconds: u64 },
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FilterRule {
    Include(String),
    Exclude(String),
    SizeGreaterThan(u64),
    SizeLessThan(u64),
    ModifiedAfter(i64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionConfig {
    pub algorithm: EncryptionAlgorithm,
    pub key_id: String,
    pub iv_mode: IvMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiffMode {
    Full,
    Incremental,
    Smart,
}

pub struct ConfigManager {
    config_path: PathBuf,
    accounts: HashMap<String, AccountConfig>,
    tasks: HashMap<String, SyncTask>,
}

impl ConfigManager {
    pub fn get_task(&self, task_id: &str) -> Option<SyncTask> {
        self.tasks.get(task_id).cloned()
    }
}

impl ConfigManager {
    pub fn new() -> Result<Self, ConfigError> {
        let config_dir = dirs::config_dir()
            .ok_or(ConfigError::NoConfigDir)?
            .join("disksync");

        // todo 进行错误处理！
        create_dir_all(&config_dir).unwrap();

        let config_path = config_dir.join("config.yaml");

        let mut manager = Self {
            config_path,
            accounts: HashMap::new(),
            tasks: HashMap::new(),
        };

        manager.load()?;
        Ok(manager)
    }

    pub fn load(&mut self) -> Result<(), ConfigError> {
        if self.config_path.exists() {
            let content = fs::read_to_string(&self.config_path).unwrap();
            let config: ConfigFile = serde_yaml::from_str(&content).unwrap();
            self.accounts = config.accounts.into_iter()
                .map(|a| (a.id.clone(), a))
                .collect();
            self.tasks = config.tasks.into_iter()
                .map(|t| (t.id.clone(), t))
                .collect();
        }
        Ok(())
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        let config = ConfigFile {
            version: "".to_string(),
            global_settings: Default::default(),
            accounts: self.accounts.values().cloned().collect(),
            tasks: self.tasks.values().cloned().collect(),
            encryption_keys: vec![],
            plugins: vec![],
            schedules: vec![],
            network_settings: None,
            security_settings: None,
        };

        // 写入配置信息到文件！
        let content = serde_yaml::to_string(&config).unwrap();
        fs::write(&self.config_path, content).unwrap();
        Ok(())
    }
}

impl ConfigManager {
    pub fn get_tasks(&self) -> &HashMap<String, SyncTask> {
        &self.tasks
    }

    pub fn get_accounts(&self) -> &HashMap<String, AccountConfig> {
        &self.accounts
    }

    pub fn add_task(&mut self, task: SyncTask) -> Result<(), ConfigError> {
        self.tasks.insert(task.id.clone(), task);
        Ok(())
    }

    pub fn add_account(&mut self, account: AccountConfig) -> Result<(), ConfigError> {
        self.accounts.insert(account.id.clone(), account);
        Ok(())
    }
}

// 主要配置文件结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigFile {
    pub version: String,
    pub global_settings: GlobalSettings,
    pub accounts: Vec<AccountConfig>,
    pub tasks: Vec<SyncTask>,
    pub encryption_keys: Vec<EncryptionKey>,
    pub plugins: Vec<PluginConfig>,
    pub schedules: Vec<ScheduleConfig>,
    pub network_settings: Option<NetworkSettings>,
    pub security_settings: Option<SecuritySettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalSettings {
    pub data_dir: Option<PathBuf>,
    pub temp_dir: Option<PathBuf>,
    pub log_level: LogLevel,
    pub log_retention_days: u32,
    pub max_concurrent_tasks: usize,
    pub default_retry_policy: RetryPolicy,
    pub enable_telemetry: bool,
    pub auto_update_check: bool,
    pub ui_language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LogLevel {
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

// todo 这里的字段不完善，lastUsed，createTime等字段不完整
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionKey {
    pub id: String,
    pub name: String,
    pub algorithm: EncryptionAlgorithm,
    pub key_data: KeyData,
    pub description: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KeyData {
    /// 本地存储的密钥（加密存储）
    Local { encrypted_data: Vec<u8>, salt: Vec<u8> },
    /// 外部密钥管理服务
    External { service: String, key_uri: String },
    /// 硬件安全模块
    HSM { module_id: String, key_handle: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    pub name: String,
    pub enabled: bool,
    pub version: String,
    pub source: PluginSource,
    pub config: HashMap<String, serde_json::Value>,
    pub hooks: Vec<PluginHookConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PluginSource {
    /// 内置插件
    Builtin,
    /// 本地文件
    Local { path: PathBuf },
    /// Git仓库
    Git { url: String, branch: Option<String> },
    /// 注册表
    Registry { name: String, version: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginHookConfig {
    pub hook_name: String,
    pub priority: i32,
    pub enabled: bool,
    pub config: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleConfig {
    pub id: String,
    pub name: String,
    pub schedule: Schedule,
    pub task_ids: Vec<String>,
    pub enabled: bool,
    pub max_runtime: Option<u64>, // 秒
    pub overlap_policy: OverlapPolicy,
    pub notifications: Vec<NotificationConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OverlapPolicy {
    /// 允许重叠执行
    Allow,
    /// 跳过新的执行
    Skip,
    /// 终止当前执行
    Terminate,
    /// 排队等待
    Queue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    pub type_: NotificationType,
    pub destination: String,
    pub events: Vec<NotificationEvent>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotificationType {
    Email,
    Webhook,
    Slack,
    Discord,
    Telegram,
    Pushover,
    Custom { command: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotificationEvent {
    TaskStarted,
    TaskCompleted,
    TaskFailed,
    TaskCancelled,
    DiskFull,
    RateLimited,
    SecurityAlert,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkSettings {
    pub proxy: Option<ProxyConfig>,
    pub dns_servers: Vec<String>,
    pub timeout_seconds: u64,
    pub connection_pool_size: usize,
    pub enable_compression: bool,
    pub enable_caching: bool,
    pub user_agent: Option<String>,
    pub custom_headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    pub url: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub bypass_for_local: bool,
    pub bypass_list: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecuritySettings {
    pub enable_audit_log: bool,
    pub audit_log_retention_days: u32,
    pub enable_two_factor_auth: bool,
    pub session_timeout_minutes: u32,
    pub ip_whitelist: Vec<String>,
    pub ip_blacklist: Vec<String>,
    pub allowed_countries: Vec<String>,
    pub block_tor_connections: bool,
    pub rate_limiting: RateLimitingSettings,
    pub encryption: SecurityEncryptionSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitingSettings {
    pub max_requests_per_minute: u32,
    pub max_connections_per_ip: u32,
    pub burst_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityEncryptionSettings {
    pub default_algorithm: EncryptionAlgorithm,
    pub key_rotation_days: u32,
    pub enforce_encryption: bool,
    pub secure_key_storage: bool,
}

impl ConfigFile {
    pub fn new() -> Self {
        Self {
            version: "1.0.0".to_string(),
            global_settings: GlobalSettings::default(),
            accounts: Vec::new(),
            tasks: Vec::new(),
            encryption_keys: Vec::new(),
            plugins: Vec::new(),
            schedules: Vec::new(),
            network_settings: Some(NetworkSettings::default()),
            security_settings: Some(SecuritySettings::default()),
        }
    }

    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        // 验证版本
        if self.version != "1.0.0" {
            errors.push(format!("Unsupported config version: {}", self.version));
        }

        // 验证账户
        for (i, account) in self.accounts.iter().enumerate() {
            if let Err(err) = account.validate() {
                errors.push(format!("Account {} (index {}): {}", account.name, i, err));
            }
        }

        // 验证任务
        for (i, task) in self.tasks.iter().enumerate() {
            if let Err(err) = task.validate() {
                errors.push(format!("Task {} (index {}): {}", task.name, i, err));
            }
        }

        // 验证密钥
        let mut key_ids = std::collections::HashSet::new();
        for key in &self.encryption_keys {
            if key_ids.contains(&key.id) {
                errors.push(format!("Duplicate encryption key ID: {}", key.id));
            }
            key_ids.insert(key.id.clone());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    pub fn find_account(&self, account_id: &str) -> Option<&AccountConfig> {
        self.accounts.iter().find(|a| a.id == account_id)
    }

    pub fn find_task(&self, task_id: &str) -> Option<&SyncTask> {
        self.tasks.iter().find(|t| t.id == task_id)
    }

    pub fn find_encryption_key(&self, key_id: &str) -> Option<&EncryptionKey> {
        self.encryption_keys.iter().find(|k| k.id == key_id)
    }

    pub fn find_schedule(&self, schedule_id: &str) -> Option<&ScheduleConfig> {
        self.schedules.iter().find(|s| s.id == schedule_id)
    }
}

impl Default for GlobalSettings {
    fn default() -> Self {
        Self {
            data_dir: dirs::data_dir().map(|p| p.join("disksync")),
            temp_dir: std::env::temp_dir().to_path_buf().into(),
            log_level: LogLevel::Info,
            log_retention_days: 30,
            max_concurrent_tasks: 5,
            default_retry_policy: RetryPolicy::default(),
            enable_telemetry: false,
            auto_update_check: true,
            ui_language: "en".to_string(),
        }
    }
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self {
            proxy: None,
            dns_servers: vec!["8.8.8.8".to_string(), "8.8.4.4".to_string()],
            timeout_seconds: 30,
            connection_pool_size: 10,
            enable_compression: true,
            enable_caching: true,
            user_agent: Some(format!("DiskSync/{}", env!("CARGO_PKG_VERSION"))),
            custom_headers: HashMap::new(),
        }
    }
}

impl Default for SecuritySettings {
    fn default() -> Self {
        Self {
            enable_audit_log: true,
            audit_log_retention_days: 90,
            enable_two_factor_auth: false,
            session_timeout_minutes: 60,
            ip_whitelist: Vec::new(),
            ip_blacklist: Vec::new(),
            allowed_countries: Vec::new(),
            block_tor_connections: true,
            rate_limiting: RateLimitingSettings::default(),
            encryption: SecurityEncryptionSettings::default(),
        }
    }
}

impl Default for RateLimitingSettings {
    fn default() -> Self {
        Self {
            max_requests_per_minute: 60,
            max_connections_per_ip: 10,
            burst_size: 5,
        }
    }
}

impl Default for SecurityEncryptionSettings {
    fn default() -> Self {
        Self {
            default_algorithm: EncryptionAlgorithm::Aes256Gcm,
            key_rotation_days: 90,
            enforce_encryption: false,
            secure_key_storage: true,
        }
    }
}
