use crate::config::{AccountConfig, ConfigManager, ProviderType, RateLimitConfig, RetryPolicy};
use crate::providers::StorageProvider;
use crate::services::account_service::verify_account_connection;
use crate::services::provider_factory::create_provider;
use crate::utils::account::find_account_id;
use dialoguer::{Input, Password, Select};
use prettytable::{Table, row};
use std::collections::HashMap;

pub async fn cmd_add_account(
    config_manager: &mut ConfigManager,
    name: String,
    provider_str: String,
    token: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔄 添加新的网盘账户...");

    // 解析提供商类型
    let provider_str = if provider_str.is_empty() {
        let providers = vec!["AliYunDrive", "WebDAV", "115", "Quark"];
        let selection = Select::new()
            .with_prompt("请选择存储提供商")
            .items(&providers)
            .default(0)
            .interact()?;
        providers[selection].to_string()
    } else {
        provider_str
    };

    let provider = match provider_str.to_lowercase().as_str() {
        "aliyun" | "aliyundrive" | "阿里云盘" => ProviderType::AliYunDrive,
        "115" | "115网盘" => ProviderType::OneOneFive,
        "quark" | "夸克网盘" => ProviderType::Quark,
        "webdav" => ProviderType::WebDAV,
        _ => {
            return Err(format!("不支持的提供商: {}", provider_str).into());
        }
    };

    let mut credentials = HashMap::new();

    // 根据提供商类型收集凭证
    match provider {
        ProviderType::AliYunDrive => {
            println!("📝 添加阿里云盘账户");

            let refresh_token = if let Some(t) = token {
                t
            } else {
                Input::<String>::new()
                    .with_prompt("请输入 refresh_token")
                    .interact_text()?
            };
            credentials.insert("refresh_token".to_string(), refresh_token);
        }
        ProviderType::WebDAV => {
            println!("📝 添加 WebDAV 账户");

            let url = Input::<String>::new()
                .with_prompt("WebDAV 服务器地址 (例如: https://dav.example.com)")
                .interact_text()?;

            let username = Input::<String>::new()
                .with_prompt("用户名")
                .interact_text()?;

            let password = Password::new().with_prompt("密码").interact()?;

            credentials.insert("url".to_string(), url);
            credentials.insert("username".to_string(), username);
            credentials.insert("password".to_string(), password);
        }
        ProviderType::OneOneFive => {
            println!("📝 添加 115 网盘账户");
            println!("📱 请使用 115 App 扫描下方二维码进行授权：");
            println!("(注意：当前为演示模式，请扫描后按回车继续)");

            // 生成并显示二维码
            let qr_url = "https://115.com/s/sw/test_qr_code";
            qr2term::print_qr(qr_url).unwrap();

            println!("\n🔗 授权链接: {}", qr_url);
            println!("等待授权中... (按回车键模拟授权完成)");

            let _ = Input::<String>::new().allow_empty(true).interact_text()?;

            let cookie = if let Some(t) = token {
                t
            } else {
                // 模拟获取到的 Token/Cookie
                println!("✅ 授权成功！(模拟)");
                Input::<String>::new()
                    .with_prompt("请输入 115 网盘的 Cookie (由于是演示，请手动输入)")
                    .interact_text()?
            };

            credentials.insert("cookie".to_string(), cookie);
        }
        ProviderType::Quark => {
            println!("📝 添加夸克网盘账户");

            let cookie = if let Some(t) = token {
                t
            } else {
                Input::<String>::new()
                    .with_prompt("请输入夸克网盘的 Cookie")
                    .interact_text()?
            };

            credentials.insert("cookie".to_string(), cookie);
        }
        _ => {
            println!("ℹ️  该提供商需要手动配置");
            println!("请在配置文件中手动添加凭证信息");
        }
    }

    // 配置限流策略
    let mut rate_limit = None;
    if dialoguer::Confirm::new()
        .with_prompt("是否配置限流策略? (推荐)")
        .default(true)
        .interact()?
    {
        let requests_per_minute = Input::<u32>::new()
            .with_prompt("每分钟请求限制")
            .default(60)
            .interact_text()?;

        let max_concurrent = Input::<usize>::new()
            .with_prompt("最大并发数")
            .default(5)
            .interact_text()?;

        rate_limit = Some(RateLimitConfig {
            requests_per_minute,
            max_concurrent,
            chunk_size: 1024 * 1024, // 1MB
        });
    }

    // 生成账户ID
    let account_id = format!("{}_{}", provider_str.to_lowercase(), uuid::Uuid::new_v4());

    let account = AccountConfig {
        id: account_id.clone(),
        provider,
        name,
        credentials,
        rate_limit,
        retry_policy: RetryPolicy {
            max_retries: 3,
            initial_delay_ms: 1000,
            max_delay_ms: 10000,
            backoff_factor: 2.0,
        },
    };

    // 验证账户连接
    println!("🔗 正在验证账户连接...");

    match verify_account_connection(&account).await {
        Ok(_) => {
            println!("✅ 账户验证成功!");

            // 保存账户配置
            config_manager.add_account(account)?;
            config_manager.save()?;

            println!("📁 账户已保存，ID: {}", account_id);
            println!("💡 使用命令 `cloud-disk-sync account list` 查看所有账户");
        }
        Err(e) => {
            eprintln!("❌ 账户验证失败: {}", e);
            if !dialoguer::Confirm::new()
                .with_prompt("是否仍要保存账户配置?")
                .default(false)
                .interact()?
            {
                return Ok(());
            }

            config_manager.add_account(account)?;
            config_manager.save()?;
            println!("⚠️  账户已保存但未通过验证，请检查配置");
        }
    }

    Ok(())
}

pub fn cmd_list_accounts(config_manager: &ConfigManager) -> Result<(), Box<dyn std::error::Error>> {
    println!("👤 账户列表:");
    let accounts = config_manager.get_accounts();

    if accounts.is_empty() {
        println!("  暂无账户");
        println!("💡 使用 `cloud-disk-sync account create` 添加新账户");
        return Ok(());
    }

    let mut account_table = Table::new();
    account_table.add_row(row!["标识", "名称", "类型", "状态"]);

    for account in accounts.values() {
        let status = "✅ 已配置";
        account_table.add_row(row![
            &account.id,
            &account.name,
            format!("{:?}", account.provider),
            status
        ]);
    }

    account_table.printstd();

    Ok(())
}

pub fn cmd_remove_account(
    config_manager: &mut ConfigManager,
    id_or_name: &str,
    force: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let id = find_account_id(config_manager, id_or_name)
        .ok_or_else(|| format!("未找到账户: {}", id_or_name))?;

    let account = config_manager
        .get_account(&id)
        .ok_or_else(|| format!("账户不存在: {}", id))?;

    let account_name = account.name.clone();

    // Check if any tasks are using this account
    let tasks = config_manager.get_tasks();
    let mut used_by = Vec::new();
    for task in tasks.values() {
        if task.source_account == id || task.target_account == id {
            used_by.push(format!("{} ({})", task.name, task.id));
        }
    }

    if !used_by.is_empty() {
        eprintln!("⚠️  该账户正在被以下任务使用:");
        for task in used_by {
            eprintln!("  - {}", task);
        }
        return Err("无法删除: 账户正在使用中".into());
    }

    let confirm_msg = format!("确定要删除账户 '{}' (ID: {}) 吗?", account_name, id);

    if force
        || dialoguer::Confirm::new()
            .with_prompt(confirm_msg)
            .default(false)
            .interact()?
    {
        config_manager.remove_account(&id)?;
        config_manager.save()?;
        println!("✅ 账户已删除: {}", id);
    } else {
        println!("❌ 操作已取消");
    }
    Ok(())
}

pub async fn cmd_update_account(
    config_manager: &mut ConfigManager,
    id_or_name: &str,
    new_name: Option<String>,
    new_token: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let id = find_account_id(config_manager, id_or_name)
        .ok_or_else(|| format!("未找到账户: {}", id_or_name))?;

    let mut account = config_manager
        .get_account(&id)
        .ok_or_else(|| format!("账户不存在: {}", id))?
        .clone();

    let mut changed = false;

    if let Some(name) = new_name {
        println!("重命名账户: {} -> {}", account.name, name);
        account.name = name;
        changed = true;
    }

    if let Some(ref token) = new_token {
        println!("更新账户凭证...");
        match account.provider {
            ProviderType::AliYunDrive => {
                account
                    .credentials
                    .insert("refresh_token".to_string(), token.clone());
            }
            ProviderType::OneOneFive | ProviderType::Quark => {
                account
                    .credentials
                    .insert("cookie".to_string(), token.clone());
            }
            _ => {
                return Err("当前仅支持更新阿里云盘、115和夸克网盘的 Token/Cookie".into());
            }
        }
        changed = true;
    }

    if changed {
        if new_token.is_some() {
            println!("🔗 正在验证新凭证...");
            verify_account_connection(&account).await?;
            println!("✅ 验证成功!");
        }

        config_manager.add_account(account)?;
        config_manager.save()?;
        println!("✅ 账户已更新");
    } else {
        println!("ℹ️  没有变更");
    }

    Ok(())
}

pub async fn cmd_account_status(
    config_manager: &ConfigManager,
    id_or_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let id = find_account_id(config_manager, id_or_name)
        .ok_or_else(|| format!("未找到账户: {}", id_or_name))?;

    let account = config_manager
        .get_account(&id)
        .ok_or_else(|| format!("账户不存在: {}", id))?;

    println!("🔍 正在检查账户状态: {} ({})", account.name, id);
    println!("   类型: {:?}", account.provider);

    match verify_account_connection(&account).await {
        Ok(_) => {
            println!("✅ 状态: 正常 (连接成功)");
        }
        Err(e) => {
            println!("❌ 状态: 异常 (连接失败)");
            println!("   错误: {}", e);
        }
    }

    Ok(())
}

pub async fn cmd_browse_account(
    config_manager: &ConfigManager,
    id_or_name: &str,
    path: String,
    _recursive: bool,
    _detail: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let id = find_account_id(config_manager, id_or_name)
        .ok_or_else(|| format!("未找到账户: {}", id_or_name))?;

    let account = config_manager.get_account(&id).ok_or("Account not found")?;

    println!("正在连接账户 {}...", account.name);
    let provider = create_provider(&account).await?;

    // Convert Box<dyn StorageProvider> to Arc<dyn StorageProvider>
    let provider: std::sync::Arc<dyn StorageProvider> = std::sync::Arc::from(provider);

    crate::cli::browse::run_browse_tui(provider, path).await?;

    Ok(())
}
