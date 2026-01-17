use crate::cli::ReportCommands;
use crate::sync::engine::SyncEngine;
use dialoguer::{Select, theme::ColorfulTheme};
use prettytable::{Table, row};

pub async fn cmd_report(
    task_id: &str,
    report_id: Option<&str>,
    command: &Option<ReportCommands>,
    json_output: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let engine = SyncEngine::new().await?;

    // 如果指定了 report_id，直接显示详情
    if let Some(rid) = report_id {
        let report = engine.get_report(rid)?;
        if json_output {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            println!("{}", report.statistics.detailed_report());
        }
        return Ok(());
    }

    // 处理子命令
    if let Some(cmd) = command {
        match cmd {
            ReportCommands::List { page, limit } => {
                let reports = engine.list_reports(task_id, *limit, page * limit)?;

                if json_output {
                    // JSON 输出需要构造一个结构
                    let json_reports: Vec<serde_json::Value> = reports
                        .iter()
                        .map(|(id, start, status, duration)| {
                            serde_json::json!({
                                "report_id": id,
                                "start_time": chrono::DateTime::from_timestamp(*start, 0).unwrap().to_rfc3339(),
                                "status": status,
                                "duration_seconds": duration
                            })
                        })
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&json_reports)?);
                } else {
                    let mut table = Table::new();
                    table.set_format(*prettytable::format::consts::FORMAT_NO_BORDER_LINE_SEPARATOR);
                    table.add_row(row!["Report ID", "Time", "Status", "Duration"]);

                    for (id, start, status, duration) in reports {
                        let time_str = chrono::DateTime::from_timestamp(start, 0)
                            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                            .unwrap_or_else(|| "Unknown".to_string());

                        table.add_row(row![id, time_str, status, format!("{}s", duration)]);
                    }
                    table.printstd();
                }
            }
        }
        return Ok(());
    }

    // 交互模式：列出最近的报告供选择
    if !json_output {
        let reports = engine.list_reports(task_id, 20, 0)?;
        if reports.is_empty() {
            println!("No reports found for task: {}", task_id);
            return Ok(());
        }

        let items: Vec<String> = reports
            .iter()
            .map(|(id, start, status, duration)| {
                let time_str = chrono::DateTime::from_timestamp(*start, 0)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                    .unwrap_or_else(|| "Unknown".to_string());
                format!("{} - {} ({}) - {}s", time_str, status, id, duration)
            })
            .collect();

        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Select a report to view details")
            .default(0)
            .items(&items)
            .interact()?;

        let (selected_id, _, _, _) = &reports[selection];
        let report = engine.get_report(selected_id)?;
        println!("{}", report.statistics.detailed_report());
    } else {
        // JSON 模式下如果不提供子命令，默认列出第一页
        let reports = engine.list_reports(task_id, 20, 0)?;
        let json_reports: Vec<serde_json::Value> = reports
            .iter()
            .map(|(id, start, status, duration)| {
                serde_json::json!({
                    "report_id": id,
                    "start_time": chrono::DateTime::from_timestamp(*start, 0).unwrap().to_rfc3339(),
                    "status": status,
                    "duration_seconds": duration
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_reports)?);
    }

    Ok(())
}
