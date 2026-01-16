use crate::config::ConfigManager;
use crate::sync::engine::SyncEngine;
use crate::utils::format_bytes;
use indicatif::{ProgressBar, ProgressStyle};

pub async fn cmd_verify_integrity(
    task_id: &str,
    verify_all: bool,
) -> Result<(), Box<dyn std::error::Error>> {
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
