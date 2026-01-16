use crate::config::ConfigManager;
use crate::services::provider_factory::create_provider;
use crate::sync::engine::SyncEngine;
use crate::utils::format_bytes;
use crate::utils::task::find_task_id;
use indicatif::{ProgressBar, ProgressStyle};
use prettytable::{Table, format, row};
use std::time::Duration;

pub async fn cmd_diff_task(
    config_manager: &ConfigManager,
    id_or_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
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

        let (action_str, _color) = match file.action {
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

        table.add_row(row![file.path, source_status, action_str, target_status]);
    }

    table.printstd();

    Ok(())
}
