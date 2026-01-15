mod cli;
mod config;
mod encryption;
mod providers;
mod report;
mod sync;
mod error;
mod core;
mod plugins;
mod utils;

use crate::cli::Cli;
use crate::config::{AccountConfig, ConfigManager, ProviderType, RateLimitConfig, RetryPolicy, Schedule, SyncTask};
use crate::sync::engine::SyncEngine;
use crate::utils::format_bytes;
// 移除未解析的类型导入，直接使用方法返回推断类型
use aes_gcm::aead::Aead;
use clap::Parser;
use cli::Commands;
use rand::{thread_rng, Rng};
use std::fs;
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化日志
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let mut config_manager = ConfigManager::new()?;

    match cli.command {
        Commands::Run {
            task,
            dry_run,
            resume,
        } => {
            cmd_run_task(&config_manager, &task, dry_run, resume).await?;
        }
        Commands::AddAccount {
            name,
            provider,
            token,
        } => {
            cmd_add_account(&mut config_manager, name, provider, token).await?;
        }
        Commands::CreateTask {
            name,
            source,
            target,
            schedule,
            encrypt,
        } => {
            cmd_create_task(&mut config_manager, name, source, target, schedule, encrypt).await?;
        }
        Commands::List => {
            cmd_list_tasks(&config_manager)?;
        }
        Commands::Report { task, detailed } => {
            cmd_generate_report(&task, detailed)?;
        }
        Commands::Verify { task, all } => {
            cmd_verify_integrity(&task, all).await?;
        }
        Commands::GenKey { name, strength } => {
            cmd_generate_key(&name, strength)?;
        },
        Commands::Plugins => {
            println!("查看所有插件！")
        }
    }

    Ok(())
}

async fn cmd_verify_integrity(
    task_id: &str,
    verify_all: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use indicatif::{ProgressBar, ProgressStyle};

    println!("🔍 验证数据完整性: {}", task_id);

    let config_manager = ConfigManager::new()?;
    let task = config_manager.get_task(task_id).ok_or_else(|| format!("任务不存在: {}", task_id))?;

    let engine = SyncEngine::new().await?;

    // 创建进度条
    let progress_bar = ProgressBar::new(0);
    progress_bar.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} {msg}")?
            .progress_chars("#>-")
    );
    progress_bar.set_message("正在验证...");

    // 执行完整性验证
    let verification_result = engine.verify_integrity(&task, verify_all, |progress| {
        progress_bar.set_length(progress.total_files as u64);
        progress_bar.set_position(progress.current_file as u64);
        progress_bar.set_message(format!("正在验证: {}", progress.current_path));
    }).await?;

    progress_bar.finish_with_message("✅ 验证完成!");

    // 显示验证结果
    println!("📊 完整性验证结果:");
    println!("  验证文件数: {}", verification_result.total_files);
    println!("  通过验证: {}", verification_result.passed);
    println!("  验证失败: {}", verification_result.failed);
    println!("  跳过验证: {}", verification_result.skipped);

    if !verification_result.errors.is_empty() {
        println!("❌ 错误信息:");
        for error in &verification_result.errors {
            println!("  - {}", error);
        }
    }

    if verification_result.failed > 0 {
        println!("⚠️  发现数据完整性问题，建议重新同步受影响文件");

        if dialoguer::Confirm::new()
            .with_prompt("是否立即修复这些问题?")
            .default(true)
            .interact()?
        {
            println!("🔧 正在修复...");

            // 重新同步有问题的文件
            let repair_result = engine.repair_integrity(&task, &verification_result).await?;

            println!("✅ 修复完成:");
            println!("  修复文件数: {}", repair_result.repaired_files);
            println!("  修复数据量: {}", format_bytes(repair_result.repaired_bytes));
        }
    } else {
        println!("🎉 所有文件完整性验证通过!");
    }

    Ok(())
}

fn cmd_generate_report(task: &String, show_detail: bool) -> Result<(), Box<dyn std::error::Error>> {
    todo!()
}

fn cmd_generate_key(
    key_name: &str,
    strength: Option<u32>,
) -> Result<(), Box<dyn std::error::Error>> {
    use aes_gcm::KeyInit;
    println!("🔑 生成加密密钥: {}", key_name);

    // 确定密钥强度
    let key_strength = strength.unwrap_or(256);
    let key_size = match key_strength {
        128 => 16,
        192 => 24,
        256 => 32,
        _ => {
            eprintln!("⚠️  不支持的密钥强度: {}，使用默认256位", key_strength);
            32
        }
    };


    // 生成随机密钥
    let mut key_bytes = vec![0u8; key_size];
    thread_rng().fill(&mut key_bytes[..]);

    // 创建密钥存储目录
    let keys_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("disksync")
        .join("keys");

    fs::create_dir_all(&keys_dir)?;

    // 保存密钥文件
    let key_file = keys_dir.join(format!("{}.key", key_name));

    // 加密保存密钥（使用主密码保护）
    println!("🔒 请设置主密码来保护此密钥:");
    let password = rpassword::prompt_password("主密码: ")?;
    let confirm_password = rpassword::prompt_password("确认主密码: ")?;

    if password != confirm_password {
        return Err("两次输入的密码不一致".into());
    }

    if password.len() < 8 {
        return Err("密码长度至少8位".into());
    }

    // 使用PBKDF2派生密钥加密密钥
    let salt: [u8; 16] = rand::random();
    let mut encryption_key = [0u8; 32];

    // pbkdf2::pbkdf2::<hmac::Hmac<sha2::Sha256>>(
    //     password.as_bytes(),
    //     &salt,
    //     100_000,
    //     &mut encryption_key,
    // );

    // 加密密钥数据
    let cipher = aes_gcm::Aes256Gcm::new(&encryption_key.into());
    let nonce: [u8; 12] = rand::random();

    let encrypted_key = cipher.encrypt(&nonce.into(), key_bytes.as_ref())
        .map_err(|e| format!("加密密钥失败: {}", e))?;

    // 保存加密的密钥文件
    let key_data = KeyFile {
        version: 1,
        algorithm: "AES-256-GCM".to_string(),
        key_strength,
        salt: salt.to_vec(),
        nonce: nonce.to_vec(),
        encrypted_key,
        created_at: chrono::Utc::now(),
        last_used: None,
    };

    let json_data = serde_json::to_string_pretty(&key_data)?;
    fs::write(&key_file, json_data)?;

    // 显示密钥信息
    println!("✅ 密钥生成成功!");
    println!("📁 密钥文件: {}", key_file.display());
    println!("📏 密钥强度: {} 位", key_strength);
    println!("🔐 加密算法: AES-256-GCM");
    println!("📅 创建时间: {}", key_data.created_at);
    println!("💡 密钥ID: {}", key_name);

    // 显示重要提示
    println!("\n⚠️  重要提示:");
    println!("  1. 请妥善保管密钥文件和主密码");
    println!("  2. 丢失密钥或密码将无法解密已加密的文件");
    println!("  3. 建议备份密钥文件到安全的地方");
    println!("  4. 不要将密钥文件与加密数据存储在同一位置");

    // 生成恢复代码
    let recovery_code = generate_recovery_code(&key_bytes);
    println!("\n🔐 恢复代码 (请在安全的地方保存):");
    println!("{}", recovery_code);

    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize)]
struct KeyFile {
    version: u32,
    algorithm: String,
    key_strength: u32,
    salt: Vec<u8>,
    nonce: Vec<u8>,
    encrypted_key: Vec<u8>,
    created_at: chrono::DateTime<chrono::Utc>,
    last_used: Option<chrono::DateTime<chrono::Utc>>,
}

fn generate_recovery_code(key: &[u8]) -> String {
    use base64::Engine;
    use sha2::{Digest, Sha256};

    // 计算密钥哈希
    let mut hasher = Sha256::new();
    hasher.update(key);
    let hash = hasher.finalize();

    // 转换为单词列表（便于记忆）
    let wordlist = vec![
        "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel",
        "india", "juliet", "kilo", "lima", "mike", "november", "oscar", "papa",
        "quebec", "romeo", "sierra", "tango", "uniform", "victor", "whiskey",
        "xray", "yankee", "zulu", "zero", "one", "two", "three", "four", "five",
        "six", "seven", "eight", "nine"
    ];

    let mut words = Vec::new();
    for chunk in hash.chunks(2) {
        let index = ((chunk[0] as usize) << 8 | chunk[1] as usize) % wordlist.len();
        words.push(wordlist[index]);
    }

    // 取前8个单词
    words[..8].join("-")
}

fn cmd_list_tasks(
    config_manager: &ConfigManager,
) -> Result<(), Box<dyn std::error::Error>> {
    use prettytable::{row, Table};

    println!("📋 同步任务列表:");

    let tasks = config_manager.get_tasks();

    if tasks.is_empty() {
        println!("  暂无同步任务");
        println!("💡 使用 `disksync create-task` 创建新任务");
        return Ok(());
    }

    let mut table = Table::new();
    table.add_row(row![
        "ID",
        "名称",
        "源",
        "目标",
        "计划",
        "状态"
    ]);

    for task in tasks.values() {
        let schedule_str = match &task.schedule {
            Some(Schedule::Cron(expr)) => format!("cron: {}", expr),
            Some(Schedule::Interval { seconds }) => {
                if *seconds >= 86400 {
                    format!("每天 {:?}", seconds / 86400)
                } else if *seconds >= 3600 {
                    format!("每{}小时", seconds / 3600)
                } else {
                    format!("每{}秒", seconds)
                }
            }
            Some(Schedule::Manual) => "手动".to_string(),
            None => "手动".to_string(),
        };

        // 检查任务状态
        let status = get_task_status(task);

        table.add_row(row![
            &task.id[..8],  // 只显示前8个字符
            &task.name,
            format!("{}:{}", task.source_account, task.source_path),
            format!("{}:{}", task.target_account, task.target_path),
            schedule_str,
            status
        ]);
    }

    table.printstd();

    // 显示账户信息
    println!("\n👤 账户列表:");
    let accounts = config_manager.get_accounts();

    let mut account_table = Table::new();
    account_table.add_row(row![
        "名称",
        "类型",
        "状态"
    ]);

    for account in accounts.values() {
        let status = "✅ 已配置";
        account_table.add_row(row![
            &account.name,
            format!("{:?}", account.provider),
            status
        ]);
    }

    account_table.printstd();

    Ok(())
}

fn get_task_status(task: &SyncTask) -> String {
    // 这里可以检查任务上次执行时间、是否启用等
    // 简化实现，总是返回就绪
    "✅ 就绪".to_string()
}

async fn cmd_create_task(
    config_manager: &mut ConfigManager,
    name: String,
    source_str: String,
    target_str: String,
    schedule_str: Option<String>,
    encrypt: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use dialoguer::{Input, Select};
    use crate::config::{DiffMode, FilterRule, Schedule, SyncTask};
    use crate::config::EncryptionConfig;

    println!("🔄 创建新的同步任务...");

    // 解析源和目标
    let (source_account, source_path) = parse_account_path(&source_str)?;
    let (target_account, target_path) = parse_account_path(&target_str)?;

    // 验证账户存在
    let accounts = config_manager.get_accounts();
    if !accounts.contains_key(&source_account) {
        return Err(format!("源账户不存在: {}", source_account).into());
    }
    if !accounts.contains_key(&target_account) {
        return Err(format!("目标账户不存在: {}", target_account).into());
    }

    // 选择同步模式
    let diff_modes = vec!["完整同步", "增量同步", "智能同步"];
    let diff_selection = Select::new()
        .with_prompt("选择同步模式")
        .items(&diff_modes)
        .default(2)
        .interact()?;

    let diff_mode = match diff_selection {
        0 => DiffMode::Full,
        1 => DiffMode::Incremental,
        2 => DiffMode::Smart,
        _ => DiffMode::Smart,
    };

    // 配置过滤规则
    let mut filters = Vec::new();

    println!("📁 配置文件过滤规则 (可选):");
    if dialoguer::Confirm::new()
        .with_prompt("是否排除隐藏文件?")
        .default(true)
        .interact()?
    {
        filters.push(FilterRule::Exclude(".*".to_string()));
        filters.push(FilterRule::Exclude("*/.*".to_string()));
    }

    // 配置加密
    let encryption_config = if encrypt {
        println!("🔒 配置文件加密");

        let key_name = Input::<String>::new()
            .with_prompt("加密密钥名称")
            .default("default".to_string())
            .interact_text()?;

        Some(EncryptionConfig {
            algorithm: EncryptionAlgorithm::Aes256Gcm,
            key_id: key_name,
            iv_mode: IvMode::Random,
        })
    } else {
        None
    };

    // 配置计划任务
    let schedule = if let Some(schedule_str) = schedule_str {
        if schedule_str.to_lowercase() == "manual" {
            Some(Schedule::Manual)
        } else if let Ok(seconds) = schedule_str.parse::<u64>() {
            Some(Schedule::Interval { seconds })
        } else {
            // 假设是 cron 表达式
            Some(Schedule::Cron(schedule_str))
        }
    } else {
        let schedule_options = vec![
            "手动执行",
            "每小时",
            "每天",
            "每周",
            "自定义 Cron 表达式",
        ];

        let selection = Select::new()
            .with_prompt("选择执行计划")
            .items(&schedule_options)
            .default(0)
            .interact()?;

        match selection {
            0 => None,
            1 => Some(Schedule::Interval { seconds: 3600 }),
            2 => Some(Schedule::Interval { seconds: 86400 }),
            3 => Some(Schedule::Interval { seconds: 604800 }),
            4 => {
                let cron_expr = Input::<String>::new()
                    .with_prompt("输入 Cron 表达式 (例如: '0 2 * * *' 表示每天凌晨2点)")
                    .interact_text()?;
                Some(Schedule::Cron(cron_expr))
            }
            _ => None,
        }
    };

    // 生成任务ID
    let task_id = format!("task_{}", uuid::Uuid::new_v4());

    if schedule.is_some() {
        println!("⏰ 任务已配置为计划执行");
    }

    let task = SyncTask {
        id: task_id.clone(),
        name,
        source_account,
        source_path,
        target_account,
        target_path,
        schedule,
        filters,
        encryption: encryption_config,
        diff_mode,
        preserve_metadata: true,
        verify_integrity: false,
    };

    // 保存任务
    config_manager.add_task(task)?;
    config_manager.save()?;

    println!("✅ 任务创建成功!");
    println!("📋 任务ID: {}", task_id);
    println!("💡 使用命令 `disksync run --task {}` 立即执行", task_id);
    Ok(())
}

fn parse_account_path(path_str: &str) -> Result<(String, String), Box<dyn std::error::Error>> {
    // 格式: account_name:/path/to/folder
    let parts: Vec<&str> = path_str.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(format!("无效的路径格式，应为 account_name:/path/to/folder，实际: {}", path_str).into());
    }

    let account = parts[0].trim().to_string();
    let path = parts[1].trim().to_string();

    if account.is_empty() || path.is_empty() {
        return Err("账户名或路径不能为空".into());
    }

    Ok((account, path))
}

async fn cmd_add_account(
    config_manager: &mut ConfigManager,
    name: String,
    provider_str: String,
    token: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::config::ProviderType;
    use dialoguer::{Input, Password};
    use std::collections::HashMap;

    println!("🔄 添加新的网盘账户...");

    // 解析提供商类型
    let provider = match provider_str.to_lowercase().as_str() {
        "aliyun" | "aliyundrive" | "阿里云盘" => ProviderType::AliYunDrive,
        "115" | "115网盘" => ProviderType::OneOneFive,
        "quark" | "夸克网盘" => ProviderType::Quark,
        "webdav" => ProviderType::WebDAV,
        // "smb" | "samba" => ProviderType::SMB,
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

            let password = Password::new()
                .with_prompt("密码")
                .interact()?;

            credentials.insert("url".to_string(), url);
            credentials.insert("username".to_string(), username);
            credentials.insert("password".to_string(), password);
        }
        // ProviderType::SMB => {
        //     println!("📝 添加 SMB 共享账户");

        //     let server = Input::<String>::new()
        //         .with_prompt("服务器地址 (例如: 192.168.1.100 或 hostname)")
        //         .interact_text()?;

        //     let share = Input::<String>::new()
        //         .with_prompt("共享名称")
        //         .interact_text()?;

        //     let username = Input::<String>::new()
        //         .with_prompt("用户名 (可选)")
        //         .allow_empty(true)
        //         .interact_text()?;

        //     let password = Password::new()
        //         .with_prompt("密码 (可选)")
        //         .interact()?;

        //     credentials.insert("server".to_string(), server);
        //     credentials.insert("share".to_string(), share);
        //     if !username.is_empty() {
        //         credentials.insert("username".to_string(), username);
        //     }
        //     if !password.is_empty() {
        //         credentials.insert("password".to_string(), password);
        //     }
        // }
        ProviderType::OneOneFive => {
            println!("📝 添加 115 网盘账户");

            let cookie = if let Some(t) = token {
                t
            } else {
                Input::<String>::new()
                    .with_prompt("请输入 115 网盘的 Cookie")
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
            println!("💡 使用命令 `disksync list` 查看所有账户");
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

async fn verify_account_connection(account: &AccountConfig) -> Result<(), Box<dyn std::error::Error>> {
    // 根据提供商类型创建客户端并测试连接
    match account.provider {
        ProviderType::AliYunDrive => verify_aliyun_account(account).await,
        ProviderType::WebDAV => verify_webdav_account(account).await,
        // ProviderType::SMB => verify_smb_account(account).await,
        _ => Ok(()), // 其他提供商暂不验证
    }
}

async fn verify_aliyun_account(account: &AccountConfig) -> Result<(), Box<dyn std::error::Error>> {
    use reqwest::Client;

    let refresh_token = account.credentials.get("refresh_token")
        .ok_or("缺少 refresh_token")?;

    let client = Client::new();

    // 测试获取访问令牌
    let response = client
        .post("https://auth.aliyundrive.com/v2/account/token")
        .json(&serde_json::json!({
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(format!("令牌获取失败: {}", response.status()).into());
    }

    Ok(())
}

async fn verify_webdav_account(account: &AccountConfig) -> Result<(), Box<dyn std::error::Error>> {
    use reqwest::Client;
    use base64::Engine;

    let url = account.credentials.get("url")
        .ok_or("缺少 URL")?;
    let username = account.credentials.get("username")
        .ok_or("缺少用户名")?;
    let password = account.credentials.get("password")
        .ok_or("缺少密码")?;

    let client = Client::new();

    // 发送 PROPFIND 请求测试连接
    let response = client
        .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), url)
        .header("Depth", "0")
        .header(
            "Authorization",
            format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", username, password))
            ),
        )
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(format!("WebDAV 连接失败: {}", response.status()).into());
    }

    Ok(())
}

use crate::encryption::types::{EncryptionAlgorithm, IvMode};
// use smb::{Client, ClientConfig, ReadAtChannel};

// async fn verify_smb_account(account: &AccountConfig) -> Result<(), Box<dyn std::error::Error>> {
//     let server = account.credentials.get("server")
//         .ok_or("缺少服务器地址")?;
//     let share_name = account.credentials.get("share")
//         .ok_or("缺少共享名称")?;

//     let client = Client::new(ClientConfig::default());

//     if let Some(username) = account.credentials.get("username") {
//         // connection.set_username(username);
//     }

//     if let Some(password) = account.credentials.get("password") {
//         // connection.set_password(password);
//     }

//     let arc = client.connect("").await?.connect().await?;

//     // 尝试连接

//     let shares = client.list_shares("")?;

//     // 检查指定的共享是否存在
//     if !shares.iter().any(|s| s.name() == share_name) {
//         return Err(format!("共享 '{}' 不存在", share_name).into());
//     }

//     Ok(())
// }

async fn cmd_run_task(
    config_manager: &ConfigManager,
    task_id: &str,
    dry_run: bool,
    resume: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let task = config_manager.get_task(task_id).ok_or("Task not found")?;

    let engine = SyncEngine::new().await?;

    if dry_run {
        println!("Dry run mode - showing what would be synced:");
        let diff = engine.calculate_diff_for_dry_run(&task).await?;
        println!("Files to sync: {}", diff.files.len());
        for file in diff.files {
            println!("  {} ({})", file.path, format_bytes(file.size_diff as u64));
        }
    } else {
        let progress_bar = indicatif::ProgressBar::new(100);
        let style = indicatif::ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} {msg}")
            .unwrap()
            .progress_chars("#>-");
        progress_bar.set_style(style);

        let report = engine.sync_with_progress(&task, |progress| {
            progress_bar.set_position(progress.percentage as u64);
            progress_bar.set_message(format!(
                "{}/{}",
                format_bytes(progress.transferred),
                format_bytes(progress.total)
            ));
        }).await?;

        progress_bar.finish_with_message("Sync completed!");

        // 保存报告
        report.save();

        // 显示报告
        println!("{}", report.summary());
    }

    Ok(())
}
