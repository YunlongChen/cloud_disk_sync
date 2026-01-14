use crate::error::Result;
use std::path::Path;

pub struct ConfigUtils;

impl ConfigUtils {
    /// 生成默认配置文件
    pub fn generate_default_config() -> String {
        let config = super::ConfigFile::new();
        serde_yaml::to_string(&config).unwrap_or_default()
    }

    /// 验证配置文件语法
    pub fn validate_config_syntax(content: &str) -> Result<()> {
        let _: super::ConfigFile = serde_yaml::from_str(content)
            .map_err(|e| crate::error::ConfigError::ParseFailed(e.to_string()))?;
        Ok(())
    }

    /// 查找配置文件
    pub fn find_config_files() -> Vec<std::path::PathBuf> {
        let mut config_files = Vec::new();

        // 检查标准配置目录
        if let Some(config_dir) = dirs::config_dir() {
            let standard_path = config_dir.join("disksync").join("config.yaml");
            if standard_path.exists() {
                config_files.push(standard_path);
            }
        }

        // 检查当前目录
        let current_dir = std::env::current_dir().unwrap_or_default();
        let local_config = current_dir.join("disksync.yaml");
        if local_config.exists() {
            config_files.push(local_config);
        }

        // 检查环境变量指定的路径
        if let Ok(env_path) = std::env::var("DISKSYNC_CONFIG") {
            let path = Path::new(&env_path);
            if path.exists() {
                config_files.push(path.to_path_buf());
            }
        }

        config_files
    }

    /// 合并多个配置文件
    pub fn merge_configs(configs: &[super::ConfigFile]) -> super::ConfigFile {
        let mut merged = super::ConfigFile::new();

        for config in configs {
            // 合并账户（按ID去重）
            for account in &config.accounts {
                if !merged.accounts.iter().any(|a| a.id == account.id) {
                    merged.accounts.push(account.clone());
                }
            }

            // 合并任务（按ID去重）
            for task in &config.tasks {
                if !merged.tasks.iter().any(|t| t.id == task.id) {
                    merged.tasks.push(task.clone());
                }
            }

            // 合并其他配置（最后出现的优先级最高）
            merged.global_settings = config.global_settings.clone();
            merged.encryption_keys = config.encryption_keys.clone();
            merged.plugins = config.plugins.clone();
            merged.schedules = config.schedules.clone();
            merged.network_settings = config.network_settings.clone();
            merged.security_settings = config.security_settings.clone();
        }

        merged
    }

    /// 从环境变量加载配置
    pub fn load_from_env() -> super::ConfigFile {
        let mut config = super::ConfigFile::new();

        // 从环境变量读取全局设置
        if let Ok(log_level) = std::env::var("DISKSYNC_LOG_LEVEL") {
            config.global_settings.log_level = match log_level.to_lowercase().as_str() {
                "off" => super::LogLevel::Off,
                "error" => super::LogLevel::Error,
                "warn" => super::LogLevel::Warn,
                "info" => super::LogLevel::Info,
                "debug" => super::LogLevel::Debug,
                "trace" => super::LogLevel::Trace,
                _ => super::LogLevel::Info,
            };
        }

        if let Ok(data_dir) = std::env::var("DISKSYNC_DATA_DIR") {
            config.global_settings.data_dir = Some(data_dir.into());
        }

        // 从环境变量读取网络设置
        if let Ok(proxy) = std::env::var("DISKSYNC_PROXY") {
            config.network_settings.get_or_insert_with(|| super::NetworkSettings::default())
                .proxy = Some(super::ProxyConfig {
                url: proxy,
                username: std::env::var("DISKSYNC_PROXY_USER").ok(),
                password: std::env::var("DISKSYNC_PROXY_PASS").ok(),
                bypass_for_local: true,
                bypass_list: Vec::new(),
            });
        }

        config
    }

    /// 生成配置模板
    pub fn generate_template(template_type: ConfigTemplate) -> String {
        match template_type {
            ConfigTemplate::Full => Self::generate_default_config(),
            ConfigTemplate::Minimal => {
                let mut config = super::ConfigFile::new();
                config.accounts.clear();
                config.tasks.clear();
                config.encryption_keys.clear();
                config.plugins.clear();
                config.schedules.clear();
                serde_yaml::to_string(&config).unwrap_or_default()
            }
            ConfigTemplate::AliYunDrive => {
                let config = r#"
accounts:
  - id: "aliyun_example"
    name: "阿里云盘示例"
    provider: AliYunDrive
    credentials:
      refresh_token: "your_refresh_token_here"
    rate_limit:
      requests_per_minute: 60
      max_concurrent: 5
      chunk_size: 4194304

tasks:
  - id: "backup_photos"
    name: "照片备份"
    source_account: "aliyun_example"
    source_path: "/相册"
    target_account: "local_backup"
    target_path: "/backup/photos"
    schedule:
      Cron: "0 2 * * *"
    filters:
      - Include: "*.jpg"
      - Include: "*.png"
      - Exclude: "thumbnails/*"
    encryption:
      algorithm: Aes256Gcm
      key_id: "master_key"
      iv_mode: Random
    diff_mode: Smart
    preserve_metadata: true

encryption_keys:
  - id: "master_key"
    name: "主密钥"
    algorithm: Aes256Gcm
    key_data:
      Local:
        encrypted_data: "base64_encrypted_key_here"
        salt: "base64_salt_here"
    created_at: "2024-01-01T00:00:00Z"
"#;
                config.to_string()
            }
            ConfigTemplate::WebDAV => {
                let config = r#"
accounts:
  - id: "webdav_example"
    name: "WebDAV服务器"
    provider: WebDAV
    credentials:
      url: "https://dav.example.com"
      username: "your_username"
      password: "your_password"
    rate_limit:
      requests_per_minute: 30
      max_concurrent: 3
      chunk_size: 2097152
"#;
                config.to_string()
            }
        }
    }

    /// 配置差异比较
    pub fn diff_configs(config1: &super::ConfigFile, config2: &super::ConfigFile) -> ConfigDiff {
        let mut diff = ConfigDiff::new();

        // 比较账户
        for account in &config1.accounts {
            if !config2.accounts.iter().any(|a| a.id == account.id) {
                diff.accounts_added.push(account.clone());
            }
        }

        for account in &config2.accounts {
            if !config1.accounts.iter().any(|a| a.id == account.id) {
                diff.accounts_removed.push(account.clone());
            }
        }

        // 比较任务
        for task in &config1.tasks {
            if !config2.tasks.iter().any(|t| t.id == task.id) {
                diff.tasks_added.push(task.clone());
            }
        }

        for task in &config2.tasks {
            if !config1.tasks.iter().any(|t| t.id == task.id) {
                diff.tasks_removed.push(task.clone());
            }
        }

        // 比较设置
        if config1.global_settings.log_level != config2.global_settings.log_level {
            diff.settings_changed.push("log_level".to_string());
        }

        diff
    }
}

pub enum ConfigTemplate {
    Full,
    Minimal,
    AliYunDrive,
    WebDAV,
}

#[derive(Debug)]
pub struct ConfigDiff {
    pub accounts_added: Vec<super::AccountConfig>,
    pub accounts_removed: Vec<super::AccountConfig>,
    pub tasks_added: Vec<super::SyncTask>,
    pub tasks_removed: Vec<super::SyncTask>,
    pub settings_changed: Vec<String>,
}

impl ConfigDiff {
    pub fn new() -> Self {
        Self {
            accounts_added: Vec::new(),
            accounts_removed: Vec::new(),
            tasks_added: Vec::new(),
            tasks_removed: Vec::new(),
            settings_changed: Vec::new(),
        }
    }

    pub fn has_changes(&self) -> bool {
        !self.accounts_added.is_empty() ||
            !self.accounts_removed.is_empty() ||
            !self.tasks_added.is_empty() ||
            !self.tasks_removed.is_empty() ||
            !self.settings_changed.is_empty()
    }
}