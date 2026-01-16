use crate::config::{ConfigManager, SyncTask};
use std::fs;

pub fn find_task_id(config_manager: &ConfigManager, id_or_name: &str) -> Option<String> {
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

pub fn get_task_status(_task: &SyncTask) -> String {
    // 这里可以检查任务上次执行时间、是否启用等
    // 简化实现，总是返回就绪
    "✅ 就绪".to_string()
}

pub fn remove_task_reports(task_id: &str) -> std::io::Result<()> {
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
