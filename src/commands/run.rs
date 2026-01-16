use crate::config::ConfigManager;
use crate::services::provider_factory::create_provider;
use crate::sync::engine::SyncEngine;
use crate::utils::format_bytes;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub async fn cmd_run_task(
    config_manager: &ConfigManager,
    task_id: &str,
    dry_run: bool,
    _resume: bool,
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
            let last_processed_file = Arc::new(Mutex::new(String::new()));

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
            let multi_progress = MultiProgress::new();

            // 1. 总体进度条 (Header) - 始终在最上方
            let main_pb = multi_progress.add(ProgressBar::new(100));
            let main_style = ProgressStyle::default_bar()
                .template("[{elapsed_precise}] ({pos}/{len}) [{bar:30.cyan/blue}] {percent}% {msg}")
                .unwrap()
                .progress_chars("=>-");
            main_pb.set_style(main_style);

            // 共享状态
            let main_pb_clone = main_pb.clone();
            let mp_clone = multi_progress.clone();

            // 跟踪当前活跃的文件进度条: (文件名, 进度条)
            let active_file = Arc::new(Mutex::new(None::<(String, ProgressBar)>));
            let active_file_clone = active_file.clone();

            // 跟踪已完成的进度条，用于限制显示数量
            let completed_bars = Arc::new(Mutex::new(VecDeque::<ProgressBar>::new()));
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
                    let new_pb = ProgressBar::new(progress.current_file_size);

                    // 获取终端宽度
                    let (term_width, _) = crossterm::terminal::size().unwrap_or((80, 24));
                    let term_width = term_width as usize;

                    // 计算文件名可用宽度
                    // 预留空间: "  " (2) + " Syncing... (100.00 MB)" (约25) + 边距 (2) = ~30
                    let available_width = term_width.saturating_sub(35).max(10);

                    let file_style = ProgressStyle::default_bar()
                        .template("  {prefix} {msg}")
                        .unwrap();
                    new_pb.set_style(file_style);

                    // 截断和对齐文件名
                    let display_name = {
                        let s = &progress.current_file;
                        let width = UnicodeWidthStr::width(s.as_str());
                        if width > available_width {
                            // 需要截断
                            let mut w = 0;
                            // 保留开头部分 (40%)
                            let keep_start_width = (available_width * 4) / 10;
                            let mut start_str = String::new();
                            for c in s.chars() {
                                let cw = UnicodeWidthChar::width(c).unwrap_or(0);
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
                                let cw = UnicodeWidthChar::width(*c).unwrap_or(0);
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
            table.set_format(*format::consts::FORMAT_BOX_CHARS);

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
