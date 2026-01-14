use super::ConfigFile;
use crate::error::Result;
use uuid::Uuid;

pub struct ConfigMigrator;

impl ConfigMigrator {
    pub fn migrate(config: &mut ConfigFile) -> Result<()> {
        let current_version = config.version.clone();

        match current_version.as_str() {
            "0.1.0" => {
                Self::migrate_from_0_1_0_to_0_2_0(config)?;
                Self::migrate_from_0_2_0_to_1_0_0(config)?;
            }
            "0.2.0" => {
                Self::migrate_from_0_2_0_to_1_0_0(config)?;
            }
            "1.0.0" => {
                // 已经是最新版本
                return Ok(());
            }
            _ => {
                return Err(crate::error::ConfigError::Invalid(
                    format!("Unsupported config version: {}", current_version)
                ).into());
            }
        }

        config.version = "1.0.0".to_string();
        Ok(())
    }

    fn migrate_from_0_1_0_to_0_2_0(config: &mut ConfigFile) -> Result<()> {
        log::info!("Migrating config from 0.1.0 to 0.2.0");

        // 为每个账户添加默认的限流配置
        for account in &mut config.accounts {
            if account.rate_limit.is_none() {
                account.rate_limit = Some(super::RateLimitConfig {
                    requests_per_minute: 60,
                    max_concurrent: 5,
                    chunk_size: 4 * 1024 * 1024, // 4MB
                });
            }
        }

        // 为每个任务添加默认的差异模式
        for task in &mut config.tasks {
            // 在旧版本中可能没有diff_mode字段
            // 使用默认值
        }

        Ok(())
    }

    fn migrate_from_0_2_0_to_1_0_0(config: &mut ConfigFile) -> Result<()> {
        log::info!("Migrating config from 0.2.0 to 1.0.0");

        // 添加缺失的默认值
        if config.global_settings.data_dir.is_none() {
            config.global_settings.data_dir = dirs::data_dir().map(|p| p.join("disksync"));
        }

        if config.network_settings.is_none() {
            config.network_settings = Some(super::NetworkSettings::default());
        }

        if config.security_settings.is_none() {
            config.security_settings = Some(super::SecuritySettings::default());
        }

        // 确保所有任务都有ID
        for (i, task) in config.tasks.iter_mut().enumerate() {
            if task.id.is_empty() {
                task.id = format!("task_{}_{}", i, Uuid::new_v4().simple());
            }
        }

        // 确保所有账户都有ID
        for (i, account) in config.accounts.iter_mut().enumerate() {
            if account.id.is_empty() {
                account.id = format!("account_{}_{}", i, Uuid::new_v4().simple());
            }
        }

        Ok(())
    }

    pub fn backup_config(config_path: &std::path::Path) -> Result<()> {
        let backup_dir = config_path.parent().unwrap().join("backups");
        if !backup_dir.exists() {
            std::fs::create_dir_all(&backup_dir)?;
        }

        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let backup_path = backup_dir.join(format!("config_backup_{}.yaml", timestamp));

        if config_path.exists() {
            std::fs::copy(config_path, &backup_path)?;
            log::info!("Config backed up to: {}", backup_path.display());
        }

        Ok(())
    }

    pub fn restore_config(config_path: &std::path::Path, backup_name: &str) -> Result<()> {
        let backup_dir = config_path.parent().unwrap().join("backups");
        let backup_path = backup_dir.join(backup_name);

        if !backup_path.exists() {
            return Err(crate::error::ConfigError::Invalid(
                format!("Backup not found: {}", backup_name)
            ).into());
        }

        // 备份当前配置
        Self::backup_config(config_path)?;

        // 恢复备份
        std::fs::copy(&backup_path, config_path)?;
        log::info!("Config restored from backup: {}", backup_name);

        Ok(())
    }

    pub fn list_backups(config_path: &std::path::Path) -> Result<Vec<String>> {
        let backup_dir = config_path.parent().unwrap().join("backups");

        if !backup_dir.exists() {
            return Ok(Vec::new());
        }

        let mut backups = Vec::new();
        for entry in std::fs::read_dir(backup_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && path.extension().map_or(false, |ext| ext == "yaml") {
                if let Some(file_name) = path.file_name() {
                    backups.push(file_name.to_string_lossy().into_owned());
                }
            }
        }

        backups.sort();
        backups.reverse(); // 最新的在前面

        Ok(backups)
    }
}