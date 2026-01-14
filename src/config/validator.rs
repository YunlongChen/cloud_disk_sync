use crate::config::{AccountConfig, EncryptionConfig, ProviderType, SyncTask};
use crate::core::traits::ConfigValidator;
use crate::error::{ConfigError, Result};

pub struct ConfigValidatorImpl;

impl ConfigValidator for ConfigValidatorImpl {
    fn validate_account(&self, account: &AccountConfig) -> Result<()> {
        if account.id.trim().is_empty() {
            return Err(ConfigError::Invalid("Account ID cannot be empty".into()).into());
        }

        if account.name.trim().is_empty() {
            return Err(ConfigError::Invalid("Account name cannot be empty".into()).into());
        }

        // 验证凭据
        match account.provider {
            ProviderType::AliYunDrive => {
                if !account.credentials.contains_key("refresh_token") {
                    return Err(ConfigError::MissingField(
                        "refresh_token for AliYunDrive".into()
                    ).into());
                }
            }
            ProviderType::WebDAV => {
                if !account.credentials.contains_key("url") {
                    return Err(ConfigError::MissingField("url for WebDAV".into()).into());
                }
                if !account.credentials.contains_key("username") {
                    return Err(ConfigError::MissingField("username for WebDAV".into()).into());
                }
            }
            ProviderType::SMB => {
                if !account.credentials.contains_key("server") {
                    return Err(ConfigError::MissingField("server for SMB".into()).into());
                }
                if !account.credentials.contains_key("share") {
                    return Err(ConfigError::MissingField("share for SMB".into()).into());
                }
            }
            _ => {} // 其他提供商可能不需要额外验证
        }

        // 验证限流配置
        if let Some(rate_limit) = &account.rate_limit {
            if rate_limit.requests_per_minute == 0 {
                return Err(ConfigError::Invalid(
                    "Requests per minute must be greater than 0".into()
                ).into());
            }

            if rate_limit.max_concurrent == 0 {
                return Err(ConfigError::Invalid(
                    "Max concurrent must be greater than 0".into()
                ).into());
            }
        }

        Ok(())
    }

    fn validate_task(&self, task: &SyncTask) -> Result<()> {
        if task.id.trim().is_empty() {
            return Err(ConfigError::Invalid("Task ID cannot be empty".into()).into());
        }

        if task.name.trim().is_empty() {
            return Err(ConfigError::Invalid("Task name cannot be empty".into()).into());
        }

        if task.source_path.trim().is_empty() {
            return Err(ConfigError::Invalid("Source path cannot be empty".into()).into());
        }

        if task.target_path.trim().is_empty() {
            return Err(ConfigError::Invalid("Target path cannot be empty".into()).into());
        }

        // 验证源和目标账户存在
        // 这里假设有方法可以获取账户列表

        // 验证计划任务格式
        if let Some(schedule) = &task.schedule {
            match schedule {
                crate::config::Schedule::Cron(expr) => {
                    // 验证cron表达式
                    if !is_valid_cron(expr) {
                        return Err(ConfigError::Invalid(
                            format!("Invalid cron expression: {}", expr)
                        ).into());
                    }
                }
                crate::config::Schedule::Interval { seconds } => {
                    if *seconds == 0 {
                        return Err(ConfigError::Invalid(
                            "Interval seconds must be greater than 0".into()
                        ).into());
                    }
                }
                crate::config::Schedule::Manual => {}
            }
        }

        Ok(())
    }

    fn validate_encryption(&self, config: &EncryptionConfig) -> Result<()> {
        if config.key_id.trim().is_empty() {
            return Err(ConfigError::Invalid("Encryption key ID cannot be empty".into()).into());
        }

        // 验证密钥存在
        // 这里假设有方法可以检查密钥

        Ok(())
    }
}

fn is_valid_cron(expr: &str) -> bool {
    // 简单的cron表达式验证
    // 实际应该使用cron解析库
    let parts: Vec<&str> = expr.split_whitespace().collect();
    parts.len() == 5
}