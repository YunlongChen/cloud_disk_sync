mod cli;
mod config;
mod core;
mod encryption;
mod error;
mod plugins;
mod providers;
mod report;
mod sync;
mod utils;

use crate::{
    cli::Cli,
    config::{
        AccountConfig, ConfigManager, ProviderType, RateLimitConfig, RetryPolicy, Schedule,
        SyncTask,
    },
    encryption::types::{EncryptionAlgorithm, IvMode},
    sync::engine::SyncEngine,
    utils::format_bytes,
};
// 移除未解析的类型导入，直接使用方法返回推断类型
use aes_gcm::aead::Aead;
use clap::Parser;
use cli::Commands;
use rand::{Rng, rng};
use std::fs;
use tracing::info;
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
            no_progress,
        } => {
            cmd_run_task(&config_manager, &task, dry_run, resume, no_progress).await?;
        }
        Commands::Account(cmd) => match cmd {
            cli::AccountCmd::Create {
                name_or_id,
                name,
                provider,
                token,
            } => {
                let account_name = name_or_id
                    .or(name)
                    .ok_or("必须提供账户名称 (使用 --name 或直接提供名称)")?;
                // provider 现在是可选的，如果在交互模式中未提供，将在 cmd_add_account 内部处理
                let provider_val = provider.unwrap_or_default();
                cmd_add_account(&mut config_manager, account_name, provider_val, token).await?;
            }
            cli::AccountCmd::List => {
                cmd_list_accounts(&config_manager)?;
            }
            cli::AccountCmd::Remove {
                id,
                name_or_id,
                force,
            } => {
                let target_id = name_or_id
                    .or(id)
                    .ok_or("必须提供账户ID或名称 (使用 --id 或直接提供名称)")?;
                cmd_remove_account(&mut config_manager, &target_id, force)?;
            }
            cli::AccountCmd::Update {
                id,
                name_or_id,
                name,
                token,
            } => {
                let target_id = name_or_id
                    .or(id)
                    .ok_or("必须提供账户ID或名称 (使用 --id 或直接提供名称)")?;
                cmd_update_account(&mut config_manager, &target_id, name, token).await?;
            }
            cli::AccountCmd::Status { id, name_or_id } => {
                let target_id = name_or_id
                    .or(id)
                    .ok_or("必须提供账户ID或名称 (使用 --id 或直接提供名称)")?;
                cmd_account_status(&config_manager, &target_id).await?;
            }
            cli::AccountCmd::Browse {
                id,
                name_or_id,
                path,
                path_pos,
                recursive,
                detail,
            } => {
                let target_id = name_or_id.or(id).ok_or("必须提供账户ID或名称")?;
                let target_path = path_pos.or(path).unwrap_or("/".to_string());

                cmd_browse_account(&config_manager, &target_id, target_path, recursive, detail)
                    .await?;
            }
        },
        Commands::Tasks(cmd) => match cmd {
            cli::TaskCmd::Create {
                name_or_id,
                name,
                source,
                target,
                schedule,
                encrypt,
            } => {
                let task_name = name_or_id.or(name).unwrap_or_default();
                cmd_create_task(
                    &mut config_manager,
                    task_name,
                    source,
                    target,
                    schedule,
                    encrypt,
                )
                .await?;
            }
            cli::TaskCmd::List => {
                cmd_list_tasks(&config_manager)?;
            }
            cli::TaskCmd::Remove {
                id,
                name_or_id,
                name,
                force,
            } => {
                // 优先使用 name_or_id，其次使用 id，最后尝试 name (deprecated)
                let target_id = name_or_id
                    .or(id)
                    .or(name)
                    .ok_or("必须提供任务ID或名称 (使用 --id 或直接提供名称)")?;
                cmd_remove_task(&mut config_manager, &target_id, force)?;
            }
        },
        Commands::Report { task, detailed } => {
            cmd_generate_report(&task, detailed)?;
        }
        Commands::Verify { task, all } => {
            cmd_verify_integrity(&task, all).await?;
        }
        Commands::GenKey { name, strength } => {
            cmd_generate_key(&name, strength)?;
        }
        Commands::Plugins => {
            println!("查看所有插件！")
        }
        Commands::Completion { shell } => {
            cmd_generate_completion(shell)?;
        }
        Commands::Diff { name_or_id, id } => {
            let target_id = name_or_id
                .or(id)
                .ok_or("必须提供任务ID或名称 (使用 --task 或直接提供名称)")?;
            cmd_diff_task(&config_manager, &target_id).await?;
        }
        Commands::Info => {
            crate::cli::info::print_info();
        }
    }

    Ok(())
}

async fn cmd_browse_account(
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

    cli::browse::run_browse_tui(provider, path).await?;

    Ok(())
}

// Remove cmd_info function
use crate::providers::{AliYunDriveProvider, StorageProvider, WebDavProvider};

async fn create_provider(
    account: &AccountConfig,
) -> Result<Box<dyn StorageProvider>, Box<dyn std::error::Error>> {
    match account.provider {
        ProviderType::AliYunDrive => {
            let provider = AliYunDriveProvider::new(account).await?;
            Ok(Box::new(provider))
        }
        ProviderType::WebDAV => {
            let provider = WebDavProvider::new(account).await?;
            Ok(Box::new(provider))
        }
        _ => Err(format!("Unsupported provider type: {:?}", account.provider).into()),
    }
}

async fn cmd_diff_task(
    config_manager: &ConfigManager,
    id_or_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use indicatif::{ProgressBar, ProgressStyle};
    use std::time::Duration;

    let id = find_task_id(config_manager, id_or_name)
        .ok_or_else(|| format!("未找到任务: {}", id_or_name))?;

    let task = config_manager
        .get_task(&id)
        .ok_or_else(|| format!("任务不存在: {}", id))?;

    println!("🔍 正在分析差异: {} ({})", &task.name, id);
    println!("   源: {}:{}", &task.source_account, &task.source_path);
    println!("   目标: {}:{}", &task.target_account, &task.target_path);

    let mut engine = SyncEngine::new().await?;

    // 注册源提供商
    let source_account = config_manager
        .get_account(&task.source_account)
        .ok_or_else(|| format!("源账户不存在: {}", task.source_account))?;

    let source_provider = create_provider(&source_account).await?;
    engine.register_provider(task.source_account.clone(), source_provider);

    // 注册目标提供商
    let target_account = config_manager
        .get_account(&task.target_account)
        .ok_or_else(|| format!("目标账户不存在: {}", task.target_account))?;

    let target_provider = create_provider(&target_account).await?;
    engine.register_provider(task.target_account.clone(), target_provider);

    // 创建一个不定长的 spinner 进度条，因为 diff 计算时间未知
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("{spinner:.blue} {msg}")?,
    );
    spinner.enable_steady_tick(Duration::from_millis(100));
    spinner.set_message("正在扫描文件列表并计算差异...");

    // 执行 dry run 获取差异
    let mut diff_result = engine.calculate_diff_for_dry_run(&task).await?;

    spinner.finish_and_clear();

    if diff_result.files.is_empty() {
        println!("✅ 目录为空或未发现任何文件。");
        return Ok(());
    }

    println!("\n📝 差异摘要:");
    println!(
        "  总文件数: {} | 需传输: {} | 需删除: {}",
        diff_result.files.len(),
        diff_result.files_to_transfer,
        diff_result.files_to_delete
    );

    println!("\n📄 文件列表详情:");

    // 使用 prettytable 格式化输出
    use prettytable::{Table, format, row};

    // 按路径排序，方便查看
    diff_result.files.sort_by(|a, b| a.path.cmp(&b.path));

    let mut table = Table::new();
    table.set_format(*format::consts::FORMAT_NO_BORDER_LINE_SEPARATOR);
    table.set_titles(row!["Path", "Source", "Action", "Target"]);

    for file in diff_result.files {
        let source_status = if let Some(info) = &file.source_info {
            format_bytes(info.size)
        } else {
            "-".to_string()
        };

        let target_status = if let Some(info) = &file.target_info {
            format_bytes(info.size)
        } else {
            "-".to_string()
        };

        let (action_str, color) = match file.action {
            crate::sync::diff::DiffAction::Upload => ("----> (New)", "g"), // Green
            crate::sync::diff::DiffAction::Update => ("----> (Upd)", "y"), // Yellow
            crate::sync::diff::DiffAction::Delete => ("  X   (Del)", "r"), // Red
            crate::sync::diff::DiffAction::Download => ("<---- (Down)", "c"), // Cyan
            crate::sync::diff::DiffAction::Conflict => ("?? Conflict", "m"), // Magenta
            crate::sync::diff::DiffAction::Move => ("----> (Mov)", "b"),   // Blue
            crate::sync::diff::DiffAction::CreateDir => ("+DIR+ (New)", "g"), // Green
            crate::sync::diff::DiffAction::Unchanged => {
                if file.tags.contains(&"target_only".to_string()) {
                    ("  |   (Ign)", "d") // Dim/Gray (Target Only)
                } else if file.tags.contains(&"skipped_overwrite".to_string()) {
                    ("  |   (Skip)", "y") // Yellow (Skipped Update)
                } else {
                    ("=====", "") // Default (Same)
                }
            }
        };

        // 由于 prettytable 的颜色支持比较基础，这里简单处理
        // 如果想支持颜色，可以使用 term 库或者 prettytable 的 color feature
        // 这里直接输出文本

        table.add_row(row![file.path, source_status, action_str, target_status]);
    }

    table.printstd();

    Ok(())
}

fn cmd_generate_completion(shell: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    use clap::CommandFactory;
    use clap_complete::{Shell, generate};
    use std::io;

    let shell_type = match shell.as_deref() {
        Some("bash") => Shell::Bash,
        Some("zsh") => Shell::Zsh,
        Some("fish") => Shell::Fish,
        Some("powershell") | Some("pwsh") => Shell::PowerShell,
        Some("elvish") => Shell::Elvish,
        _ => {
            // 如果未指定，尝试根据环境判断，或默认为 bash
            Shell::Bash
        }
    };

    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_string();
    generate(shell_type, &mut cmd, bin_name, &mut io::stdout());

    Ok(())
}

async fn cmd_verify_integrity(
    task_id: &str,
    verify_all: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use indicatif::{ProgressBar, ProgressStyle};

    println!("🔍 验证数据完整性: {}", task_id);

    let config_manager = ConfigManager::new()?;
    let task = config_manager
        .get_task(task_id)
        .ok_or_else(|| format!("任务不存在: {}", task_id))?;

    let engine = SyncEngine::new().await?;

    // 创建进度条
    let progress_bar = ProgressBar::new(0);
    progress_bar.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} {msg}",
            )?
            .progress_chars("#>-"),
    );
    progress_bar.set_message("正在验证...");

    // 执行完整性验证
    let verification_result = engine
        .verify_integrity(&task, verify_all, |progress| {
            progress_bar.set_length(progress.total_files as u64);
            progress_bar.set_position(progress.current_file as u64);
            progress_bar.set_message(format!("正在验证: {}", progress.current_path));
        })
        .await?;

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
            println!(
                "  修复数据量: {}",
                format_bytes(repair_result.repaired_bytes)
            );
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
    rng().fill(&mut key_bytes[..]);

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

    let encrypted_key = cipher
        .encrypt(&nonce.into(), key_bytes.as_ref())
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
    use sha2::{Digest, Sha256};

    // 计算密钥哈希
    let mut hasher = Sha256::new();
    hasher.update(key);
    let hash = hasher.finalize();

    // 转换为单词列表（便于记忆）
    let wordlist = vec![
        "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india",
        "juliet", "kilo", "lima", "mike", "november", "oscar", "papa", "quebec", "romeo", "sierra",
        "tango", "uniform", "victor", "whiskey", "xray", "yankee", "zulu", "zero", "one", "two",
        "three", "four", "five", "six", "seven", "eight", "nine",
    ];

    let mut words = Vec::new();
    for chunk in hash.chunks(2) {
        let index = ((chunk[0] as usize) << 8 | chunk[1] as usize) % wordlist.len();
        words.push(wordlist[index]);
    }

    // 取前8个单词
    words[..8].join("-")
}

fn cmd_list_tasks(config_manager: &ConfigManager) -> Result<(), Box<dyn std::error::Error>> {
    use prettytable::{Table, format, row};

    println!("📋 同步任务列表:");

    let tasks = config_manager.get_tasks();

    if tasks.is_empty() {
        println!("  暂无同步任务");
        println!("💡 使用 `cloud-disk-sync tasks create` 创建新任务");
        return Ok(());
    }

    let mut table = Table::new();
    // Revert to simple format as requested
    table.set_format(*format::consts::FORMAT_NO_BORDER_LINE_SEPARATOR);

    table.add_row(row!["ID", "名称", "源", "目标", "计划", "状态"]);

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

        // 截断长字符串
        let source = format!("{}:{}", task.source_account, task.source_path);
        let target = format!("{}:{}", task.target_account, task.target_path);

        // 辅助函数：截断字符串 (UTF-8 安全)
        let truncate = |s: &str, max_width: usize| -> String {
            use unicode_width::UnicodeWidthStr;
            let mut width = 0;
            let mut result = String::new();
            for c in s.chars() {
                let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
                if width + w > max_width {
                    if width + 3 <= max_width {
                        result.push_str("...");
                    }
                    break;
                }
                width += w;
                result.push(c);
            }
            result
        };

        table.add_row(row![
            &task.id[..8], // ID is ASCII safe
            truncate(&task.name, 20),
            truncate(&source, 40),
            truncate(&target, 40),
            schedule_str,
            status
        ]);
    }

    table.printstd();

    Ok(())
}

fn cmd_list_accounts(config_manager: &ConfigManager) -> Result<(), Box<dyn std::error::Error>> {
    use prettytable::{Table, row};

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

fn get_task_status(task: &SyncTask) -> String {
    // 这里可以检查任务上次执行时间、是否启用等
    // 简化实现，总是返回就绪
    "✅ 就绪".to_string()
}

fn cmd_remove_task(
    config_manager: &mut ConfigManager,
    id_or_name: &str,
    force: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let id = find_task_id(config_manager, id_or_name)
        .ok_or_else(|| format!("未找到任务: {}", id_or_name))?;

    let task_name = config_manager
        .get_task(&id)
        .map(|t| t.name.clone())
        .unwrap_or_else(|| "未知任务".to_string());

    let confirm_msg = format!(
        "确定要删除任务 '{}' (ID: {}) 吗?\n⚠️  注意: 此操作还将删除所有相关的同步报告记录",
        task_name, id
    );

    // 确认删除
    if force
        || dialoguer::Confirm::new()
            .with_prompt(confirm_msg)
            .default(false)
            .interact()?
    {
        // 1. 从配置中移除任务
        config_manager.remove_task(&id)?;
        config_manager.save()?;

        // 2. 删除关联的同步报告
        if let Err(e) = remove_task_reports(&id) {
            eprintln!("⚠️  任务已删除，但清理同步报告失败: {}", e);
        } else {
            println!("🗑️  已清理关联的同步报告");
        }

        println!("✅ 任务已删除: {}", id);
    } else {
        println!("❌ 操作已取消");
    }
    Ok(())
}

fn find_task_id(config_manager: &ConfigManager, id_or_name: &str) -> Option<String> {
    // 尝试直接作为 ID 查找
    if config_manager.get_task(id_or_name).is_some() {
        return Some(id_or_name.to_string());
    }

    // 尝试作为名称查找
    for task in config_manager.get_tasks().values() {
        if task.name == id_or_name {
            return Some(task.id.clone());
        }
    }

    None
}

fn remove_task_reports(task_id: &str) -> std::io::Result<()> {
    let reports_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("disksync")
        .join("reports");

    if !reports_dir.exists() {
        return Ok(());
    }

    // 遍历报告目录，删除包含 task_id 的文件
    // 报告文件名通常包含 task_id，例如: report_{task_id}_{timestamp}.json
    for entry in fs::read_dir(reports_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                if file_name.contains(task_id) {
                    fs::remove_file(path)?;
                }
            }
        }
    }
    Ok(())
}

async fn cmd_create_task(
    config_manager: &mut ConfigManager,
    name: String,
    source_str: Option<String>,
    target_str: Option<String>,
    schedule_str: Option<String>,
    encrypt: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::config::EncryptionConfig;
    use crate::config::{DiffMode, FilterRule, Schedule, SyncPolicy, SyncTask};
    use dialoguer::{Input, Select};

    println!("🔄 创建新的同步任务...");

    let task_name = if name.is_empty() {
        Input::<String>::new()
            .with_prompt("请输入任务名称")
            .interact_text()?
    } else {
        name
    };

    // 获取所有可用账户
    let accounts = config_manager.get_accounts();
    if accounts.is_empty() {
        return Err("暂无可用账户，请先使用 `cloud-disk-sync account create` 添加账户".into());
    }

    let account_list: Vec<(String, String)> = accounts
        .values()
        .map(|acc| (acc.id.clone(), acc.name.clone()))
        .collect();
    let account_display: Vec<String> = account_list
        .iter()
        .map(|(id, name)| format!("{} ({})", name, id))
        .collect();

    // 选择或解析源账户
    let (source_account, source_path) = if let Some(s) = source_str {
        parse_account_path_or_select(&s, &accounts, &account_list, &account_display, "源").await?
    } else {
        select_account_and_path(&accounts, &account_list, &account_display, "源").await?
    };

    // 选择或解析目标账户
    let (target_account, target_path) = if let Some(t) = target_str {
        parse_account_path_or_select(&t, &accounts, &account_list, &account_display, "目标").await?
    } else {
        select_account_and_path(&accounts, &account_list, &account_display, "目标").await?
    };

    // 验证账户存在
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
        let schedule_options = vec!["手动执行", "每小时", "每天", "每周", "自定义 Cron 表达式"];

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
        name: task_name,
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
        sync_policy: Some(SyncPolicy {
            delete_orphans: true,
            overwrite_existing: true,
            scan_cooldown_secs: 0,
        }),
    };

    // 保存任务
    config_manager.add_task(task)?;
    config_manager.save()?;

    println!("✅ 任务创建成功!");
    println!("📋 任务ID: {}", task_id);
    println!(
        "💡 使用命令 `cloud-disk-sync run --task {}` 立即执行",
        task_id
    );
    Ok(())
}

fn parse_account_path(path_str: &str) -> Result<(String, String), Box<dyn std::error::Error>> {
    // 格式: account_name:/path/to/folder
    let parts: Vec<&str> = path_str.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(format!(
            "无效的路径格式，应为 account_name:/path/to/folder，实际: {}",
            path_str
        )
        .into());
    }

    let account = parts[0].trim().to_string();
    let path = parts[1].trim().to_string();

    if account.is_empty() || path.is_empty() {
        return Err("账户名或路径不能为空".into());
    }

    Ok((account, path))
}

async fn parse_account_path_or_select(
    input: &str,
    accounts: &std::collections::HashMap<String, AccountConfig>,
    account_list: &[(String, String)],
    account_display: &[String],
    label: &str,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    // 尝试解析输入
    if let Ok((acc, path)) = parse_account_path(input) {
        // 检查账户是否存在
        let acc_id = find_account_id_internal(accounts, &acc);
        if let Some(id) = acc_id {
            return Ok((id, path));
        } else {
            // 账户不存在，可能是只提供了账户名，没有路径
            // 或者格式错误
        }
    }

    // 尝试作为账户ID或名称查找
    let acc_id = find_account_id_internal(accounts, input);
    if let Some(id) = acc_id {
        // 找到了账户，请求路径
        let path = dialoguer::Input::<String>::new()
            .with_prompt(format!("请输入{}路径", label))
            .default("/".to_string())
            .interact_text()?;
        return Ok((id, path));
    }

    // 无法解析，进入交互选择
    println!("⚠️  无法解析账户: {}", input);
    select_account_and_path(accounts, account_list, account_display, label).await
}

async fn select_account_and_path(
    accounts: &std::collections::HashMap<String, AccountConfig>,
    account_list: &[(String, String)],
    account_display: &[String],
    label: &str,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    use dialoguer::{Input, Select};

    let selection = Select::new()
        .with_prompt(format!("选择{}账户", label))
        .items(account_display)
        .default(0)
        .interact()?;

    let (account_id, _) = &account_list[selection];
    let account = accounts.get(account_id).unwrap();

    // 尝试列出目录供选择（如果支持）
    let path = match account.provider {
        // 对于支持列出目录的提供商，可以实现交互式选择
        // 目前简化为手动输入
        _ => Input::<String>::new()
            .with_prompt(format!("请输入{}路径", label))
            .default("/".to_string())
            .interact_text()?,
    };

    Ok((account_id.clone(), path))
}

fn find_account_id_internal(
    accounts: &std::collections::HashMap<String, AccountConfig>,
    id_or_name: &str,
) -> Option<String> {
    if accounts.contains_key(id_or_name) {
        return Some(id_or_name.to_string());
    }
    for acc in accounts.values() {
        if acc.name == id_or_name {
            return Some(acc.id.clone());
        }
    }
    None
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
    let provider_str = if provider_str.is_empty() {
        use dialoguer::Select;
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

            let password = Password::new().with_prompt("密码").interact()?;

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

fn find_account_id(config_manager: &ConfigManager, id_or_name: &str) -> Option<String> {
    // 尝试直接作为 ID 查找
    if config_manager.get_account(id_or_name).is_some() {
        return Some(id_or_name.to_string());
    }

    // 尝试作为名称查找
    for account in config_manager.get_accounts().values() {
        if account.name == id_or_name {
            return Some(account.id.clone());
        }
    }

    None
}

fn cmd_remove_account(
    config_manager: &mut ConfigManager,
    id_or_name: &str,
    force: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let id = find_account_id(config_manager, id_or_name)
        .ok_or_else(|| format!("未找到账户: {}", id_or_name))?;

    // 确认删除
    if force
        || dialoguer::Confirm::new()
            .with_prompt(format!("确定要删除账户 '{}' (ID: {}) 吗?", id_or_name, id))
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

async fn cmd_update_account(
    config_manager: &mut ConfigManager,
    id_or_name: &str,
    name: Option<String>,
    token: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let id = find_account_id(config_manager, id_or_name)
        .ok_or_else(|| format!("未找到账户: {}", id_or_name))?;

    let mut account = config_manager.get_account(&id).ok_or("Account not found")?; // Should exist based on find_account_id

    let mut updated = false;
    if let Some(n) = name {
        account.name = n;
        updated = true;
    }

    if let Some(t) = token {
        // 根据提供商类型更新凭证
        match account.provider {
            ProviderType::AliYunDrive => {
                account.credentials.insert("refresh_token".to_string(), t);
            }
            ProviderType::OneOneFive | ProviderType::Quark => {
                account.credentials.insert("cookie".to_string(), t);
            }
            _ => {
                println!(
                    "⚠️  直接更新令牌仅支持基于令牌的提供商 (AliYun, 115, Quark)。对于其他提供商，请重新添加账户或手动编辑配置文件。"
                );
            }
        }
        updated = true;
    }

    if updated {
        config_manager.update_account(account)?;
        config_manager.save()?;
        println!("✅ 账户已更新: {}", id);
    } else {
        println!("ℹ️  未提供更改");
    }

    Ok(())
}

async fn cmd_account_status(
    config_manager: &ConfigManager,
    id_or_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let id = find_account_id(config_manager, id_or_name)
        .ok_or_else(|| format!("未找到账户: {}", id_or_name))?;

    let account = config_manager.get_account(&id).ok_or("Account not found")?;

    println!("🔍 正在检查账户状态: {} ({})", account.name, id);

    match verify_account_connection(&account).await {
        Ok(_) => {
            println!("✅ 状态: 在线 / 有效");
        }
        Err(e) => {
            println!("❌ 状态: 错误 - {}", e);
        }
    }

    Ok(())
}

async fn verify_account_connection(
    account: &AccountConfig,
) -> Result<(), Box<dyn std::error::Error>> {
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

    let refresh_token = account
        .credentials
        .get("refresh_token")
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
    use base64::Engine;
    use reqwest::Client;

    info!("正在验证 webdav 账户");

    let url = account.credentials.get("url").ok_or("缺少 URL")?;
    let username = account.credentials.get("username").ok_or("缺少用户名")?;
    let password = account.credentials.get("password").ok_or("缺少密码")?;

    let client = Client::new();

    // 发送 PROPFIND 请求测试连接
    let response = client
        .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), url)
        .header("Depth", "0")
        .header(
            "Authorization",
            format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD
                    .encode(format!("{}:{}", username, password))
            ),
        )
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(format!("WebDAV 连接失败: {}", response.status()).into());
    }

    Ok(())
}

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
    no_progress: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let task = config_manager.get_task(task_id).ok_or("Task not found")?;

    let mut engine = SyncEngine::new().await?;

    // 注册源提供商
    let source_account = config_manager
        .get_account(&task.source_account)
        .ok_or_else(|| format!("源账户不存在: {}", task.source_account))?;

    let source_provider = create_provider(&source_account).await?;
    engine.register_provider(task.source_account.clone(), source_provider);

    // 注册目标提供商
    let target_account = config_manager
        .get_account(&task.target_account)
        .ok_or_else(|| format!("目标账户不存在: {}", task.target_account))?;

    let target_provider = create_provider(&target_account).await?;
    engine.register_provider(task.target_account.clone(), target_provider);

    if dry_run {
        println!("Dry run mode - showing what would be synced:");
        let diff = engine.calculate_diff_for_dry_run(&task).await?;
        println!("Files to sync: {}", diff.files.len());
        for file in diff.files {
            println!("  {} ({})", file.path, format_bytes(file.size_diff as u64));
        }
    } else {
        if no_progress {
            // 静默模式，只打印日志，不显示 UI
            println!("Starting sync task {} in silent mode...", task_id);

            // 使用 Arc<Mutex> 来记录上一个处理的文件，避免重复打印
            let last_processed_file = std::sync::Arc::new(std::sync::Mutex::new(String::new()));

            let report = engine
                .sync_with_progress(&task, move |progress| {
                    let mut last = last_processed_file.lock().unwrap();
                    if *last != progress.current_file {
                        // 文件切换了，说明上一个文件完成了（或者刚开始第一个文件）
                        // 打印新开始的文件
                        println!(
                            "[{}] Syncing: {} ({})",
                            chrono::Local::now().format("%H:%M:%S"),
                            progress.current_file,
                            format_bytes(progress.current_file_size)
                        );
                        *last = progress.current_file.clone();
                    }
                })
                .await?;
            println!("{}", report.summary());
        } else {
            // 使用 MultiProgress 管理多行进度条
            let multi_progress = indicatif::MultiProgress::new();

            // 1. 总体进度条 (Header) - 始终在最上方
            let main_pb = multi_progress.add(indicatif::ProgressBar::new(100));
            let main_style = indicatif::ProgressStyle::default_bar()
                .template("[{elapsed_precise}] ({pos}/{len}) [{bar:30.cyan/blue}] {percent}% {msg}")
                .unwrap()
                .progress_chars("=>-");
            main_pb.set_style(main_style);

            // 共享状态
            let main_pb_clone = main_pb.clone();
            let mp_clone = multi_progress.clone();

            // 跟踪当前活跃的文件进度条: (文件名, 进度条)
            let active_file = std::sync::Arc::new(std::sync::Mutex::new(
                None::<(String, indicatif::ProgressBar)>,
            ));
            let active_file_clone = active_file.clone();

            // 跟踪已完成的进度条，用于限制显示数量
            let completed_bars =
                std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::<
                    indicatif::ProgressBar,
                >::new()));
            let completed_bars_clone = completed_bars.clone();

            let report = engine
                .sync_with_progress(&task, move |progress| {
                    // 更新主进度条
                    main_pb_clone.set_length(100);
                    main_pb_clone.set_position(progress.percentage as u64);
                    main_pb_clone.set_message(format!(
                        "{}/{}",
                        format_bytes(progress.transferred),
                        format_bytes(progress.total)
                    ));

                    let mut active_guard = active_file_clone.lock().unwrap();
                    let mut completed_guard = completed_bars_clone.lock().unwrap();

                    // 检查是否已有活跃进度条
                    if let Some((name, pb)) = active_guard.take() {
                        if name == progress.current_file {
                            // 文件名相同，说明是该文件的"结束"回调
                            pb.finish_with_message("Done");

                            // 将完成的进度条加入历史队列
                            completed_guard.push_front(pb);

                            // 限制历史记录数量为 10
                            if completed_guard.len() > 10 {
                                if let Some(old_pb) = completed_guard.pop_back() {
                                    old_pb.finish_and_clear();
                                }
                            }

                            // 任务完成，移除活跃状态
                            return;
                        } else {
                            // 文件名不同，说明上一个文件没有正常收到"结束"回调
                            pb.finish_with_message("-");
                            completed_guard.push_front(pb);
                            if completed_guard.len() > 10 {
                                if let Some(old_pb) = completed_guard.pop_back() {
                                    old_pb.finish_and_clear();
                                }
                            }
                        }
                    }

                    // 创建新文件的进度条
                    let new_pb = indicatif::ProgressBar::new(progress.current_file_size);

                    // 获取终端宽度
                    let (term_width, _) = crossterm::terminal::size().unwrap_or((80, 24));
                    let term_width = term_width as usize;

                    // 计算文件名可用宽度
                    // 预留空间: "  " (2) + " Syncing... (100.00 MB)" (约25) + 边距 (2) = ~30
                    let available_width = term_width.saturating_sub(35).max(10);

                    let file_style = indicatif::ProgressStyle::default_bar()
                        .template("  {prefix} {msg}")
                        .unwrap();
                    new_pb.set_style(file_style);

                    // 截断和对齐文件名
                    use unicode_width::UnicodeWidthStr;
                    let display_name = {
                        let s = &progress.current_file;
                        let width = UnicodeWidthStr::width(s.as_str());
                        if width > available_width {
                            // 需要截断
                            let mut w = 0;
                            let mut result = String::new();
                            // 保留开头部分 (40%)
                            let keep_start_width = (available_width * 4) / 10;
                            let mut start_str = String::new();
                            for c in s.chars() {
                                let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
                                if w + cw > keep_start_width {
                                    break;
                                }
                                w += cw;
                                start_str.push(c);
                            }

                            // 保留结尾部分 (50%)
                            let keep_end_width = (available_width * 5) / 10;
                            let mut end_str = String::new();
                            let chars: Vec<char> = s.chars().collect();
                            let mut w_end = 0;
                            for c in chars.iter().rev() {
                                let cw = unicode_width::UnicodeWidthChar::width(*c).unwrap_or(0);
                                if w_end + cw > keep_end_width {
                                    break;
                                }
                                w_end += cw;
                                end_str.insert(0, *c);
                            }

                            format!("{}...{}", start_str, end_str)
                        } else {
                            // 需要填充
                            let padding = available_width - width;
                            format!("{}{}", s, " ".repeat(padding))
                        }
                    };

                    new_pb.set_prefix(display_name);
                    new_pb.set_message(format!(
                        "Syncing... ({})",
                        format_bytes(progress.current_file_size)
                    ));

                    // 关键：将新进度条插入到位置 1 (Main PB 之后)，实现"最新任务在最上面"的效果
                    let new_pb = mp_clone.insert(1, new_pb);

                    // 更新活跃状态
                    *active_guard = Some((progress.current_file, new_pb));
                })
                .await?;

            main_pb.finish_with_message("Sync completed!");

            // 清理最后可能残留的活跃进度条 (如果最后一次回调没触发或者出错)
            if let Some((_, pb)) = active_file.lock().unwrap().take() {
                pb.finish_with_message("Done");
            }

            // 保存报告
            report.save();

            // 显示报告 (MySQL 风格表格)
            println!("\n📊 同步报告:");
            use prettytable::{Table, format, row};
            let mut table = Table::new();
            table.set_format(*format::consts::FORMAT_NO_TITLE);

            table.add_row(row![
                "Total Files",
                "Success",
                "Failed",
                "Total Size",
                "Avg Speed",
                "Time Cost"
            ]);

            let total_files = report.statistics.total_files;
            let success = report.statistics.files_synced;
            let failed = report.statistics.files_failed;
            let total_size = format_bytes(report.statistics.total_bytes);
            let avg_speed = format!("{}/s", format_bytes(report.statistics.average_speed as u64));
            let time_cost = format!("{:.1}s", report.duration_seconds as f64);

            table.add_row(row![
                total_files,
                success,
                failed,
                total_size,
                avg_speed,
                time_cost
            ]);

            table.printstd();
        }
    }

    Ok(())
}
