use super::{ConfigFile, security::SecurityManager};
use crate::error::Result;
use uuid::Uuid;
use std::path::Path;

pub struct ConfigMigrator;

impl ConfigMigrator {
    pub fn migrate(config: &mut ConfigFile, config_dir: &Path) -> Result<()> {
        let current_version = config.version.clone();

        // 无论当前是什么版本，只要不是 0.1.0，我们就统一迁移到 0.1.0
        // 并确保凭据被加密
        if current_version != "0.1.0" {
             log::info!("Resetting/Migrating config version from {} to 0.1.0", current_version);
             
             // 确保基本配置存在
             if config.security_settings.is_none() {
                 config.security_settings = Some(super::SecuritySettings::default());
             }
             
             if config.network_settings.is_none() {
                config.network_settings = Some(super::NetworkSettings::default());
             }

             // 执行加密逻辑 (同之前的 1.1.0 逻辑)
             let security_manager = SecurityManager::new(config_dir);
             for account in &mut config.accounts {
                for (_, value) in account.credentials.iter_mut() {
                    if !value.starts_with("ENC:") {
                        *value = security_manager.encrypt(value);
                    }
                }
             }

             config.version = "0.1.0".to_string();
        }

        Ok(())
    }

    // Removed old specific migration functions as requested
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
            return Err(crate::error::ConfigError::Invalid(format!(
                "Backup not found: {}",
                backup_name
            ))
            .into());
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
