use crate::config::{
    ConfigManager, DiffMode, EncryptionConfig, FilterRule, Schedule, SyncPolicy, SyncTask,
};
use crate::encryption::types::{EncryptionAlgorithm, IvMode};
use crate::utils::interaction::{parse_account_path_or_select, select_account_and_path};
use crate::utils::task::{find_task_id, get_task_status, remove_task_reports};
use crate::utils::truncate_string;
use dialoguer::{Input, Select};
use prettytable::{Table, format, row};

pub async fn cmd_create_task(
    config_manager: &mut ConfigManager,
    name: String,
    source_str: Option<String>,
    target_str: Option<String>,
    schedule_str: Option<String>,
    encrypt: bool,
) -> Result<(), Box<dyn std::error::Error>> {
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
        parse_account_path_or_select(&s, accounts, &account_list, &account_display, "源").await?
    } else {
        select_account_and_path(accounts, &account_list, &account_display, "源").await?
    };

    // 选择或解析目标账户
    let (target_account, target_path) = if let Some(t) = target_str {
        parse_account_path_or_select(&t, accounts, &account_list, &account_display, "目标").await?
    } else {
        select_account_and_path(accounts, &account_list, &account_display, "目标").await?
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

pub fn cmd_list_tasks(config_manager: &ConfigManager) -> Result<(), Box<dyn std::error::Error>> {
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

        table.add_row(row![
            &task.id[..8], // ID is ASCII safe
            truncate_string(&task.name, 20),
            truncate_string(&source, 40),
            truncate_string(&target, 40),
            schedule_str,
            status
        ]);
    }

    table.printstd();

    Ok(())
}

pub fn cmd_remove_task(
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
