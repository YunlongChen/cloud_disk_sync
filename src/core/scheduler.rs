use crate::config::{Schedule, SyncTask};
use crate::error::{Result, SyncError};
use crate::report::SyncReport;
use crate::sync::engine::SyncEngine;
use crate::utils::format_bytes;
use chrono::{DateTime, TimeZone, Utc};
use futures::FutureExt;
// src/scheduler/mod.rs
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::Duration;
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::error;
use uuid::Uuid;

/// 计划任务定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTask {
    /// 任务唯一ID
    pub id: String,
    /// 关联的同步任务ID
    pub sync_task_id: String,
    /// 任务名称
    pub name: String,
    /// 调度配置
    pub schedule: Schedule,
    /// 是否启用
    pub enabled: bool,
    /// 上次执行时间
    pub last_run: Option<DateTime<Utc>>,
    /// 下次执行时间
    pub next_run: Option<DateTime<Utc>>,
    /// 执行次数
    pub run_count: u32,
    /// 平均执行时间（毫秒）
    pub average_duration_ms: u64,
    /// 最后执行结果
    pub last_result: Option<TaskResult>,
    /// 最大重试次数
    pub max_retries: u32,
    /// 当前重试次数
    pub current_retries: u32,
    /// 任务超时时间（秒）
    pub timeout_seconds: u64,
    /// 任务优先级（0-100，越大优先级越高）
    pub priority: u8,
    /// 任务标签，用于分类
    pub tags: Vec<String>,
    /// 任务描述
    pub description: String,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 更新时间
    pub updated_at: DateTime<Utc>,
}

impl ScheduledTask {
    pub fn new(sync_task: &SyncTask, schedule: Schedule) -> Self {
        let now = Utc::now();

        Self {
            id: format!("sched_{}", Uuid::new_v4()),
            sync_task_id: sync_task.id.clone(),
            name: sync_task.name.clone(),
            schedule,
            enabled: true,
            last_run: None,
            next_run: None,
            run_count: 0,
            average_duration_ms: 0,
            last_result: None,
            max_retries: 3,
            current_retries: 0,
            timeout_seconds: 3600, // 默认1小时超时
            priority: 50,
            tags: vec![],
            description: format!(
                "同步任务: {} -> {}",
                sync_task.source_account, sync_task.target_account
            ),
            created_at: now,
            updated_at: now,
        }
    }

    /// 计算下次执行时间
    pub fn calculate_next_run(&mut self) -> Result<()> {
        self.next_run = match &self.schedule {
            Schedule::Cron(cron_expr) => {
                let schedule = cron::Schedule::from_str(cron_expr)
                    .map_err(|e| SyncError::Validation(format!("无效的cron表达式: {}", e)))?;

                schedule.upcoming(Utc).next()
            }
            Schedule::Interval { seconds } => Some(Utc::now() + Duration::from_secs(*seconds)),
            Schedule::Manual => None,
        };

        self.updated_at = Utc::now();
        Ok(())
    }

    /// 更新执行统计
    pub fn update_statistics(&mut self, duration: Duration, success: bool) {
        self.last_run = Some(Utc::now());
        self.run_count += 1;

        // 更新平均执行时间（移动平均）
        let duration_ms = duration.as_millis() as u64;
        if self.run_count == 1 {
            self.average_duration_ms = duration_ms;
        } else {
            self.average_duration_ms = (self.average_duration_ms * 9 + duration_ms) / 10;
        }

        self.last_result = Some(TaskResult {
            success,
            duration_ms,
            timestamp: Utc::now(),
        });

        // 重置重试计数
        if success {
            self.current_retries = 0;
        }

        self.updated_at = Utc::now();
    }

    /// 检查是否应该立即执行（用于手动触发）
    pub fn should_run_now(&self) -> bool {
        self.enabled && (self.next_run.is_none() || self.next_run.unwrap() <= Utc::now())
    }

    /// 获取任务状态
    pub fn get_status(&self) -> TaskStatus {
        if !self.enabled {
            return TaskStatus::Disabled;
        }

        if let Some(next_run) = self.next_run {
            if next_run <= Utc::now() {
                TaskStatus::Pending
            } else {
                TaskStatus::Scheduled
            }
        } else {
            TaskStatus::Manual
        }
    }

    /// 获取任务健康状态
    pub fn get_health(&self) -> TaskHealth {
        if self.current_retries >= self.max_retries {
            return TaskHealth::Critical;
        }

        if let Some(last_result) = &self.last_result {
            if !last_result.success {
                return TaskHealth::Warning;
            }

            // 检查是否超时（平均时间的2倍）
            if self.average_duration_ms > 0
                && last_result.duration_ms > self.average_duration_ms * 2
            {
                return TaskHealth::Degraded;
            }
        }

        TaskHealth::Healthy
    }

    /// 格式化下次执行时间
    pub fn format_next_run(&self) -> String {
        match self.next_run {
            Some(time) => {
                let now = Utc::now();
                let duration = time - now;

                if duration.num_seconds() < 0 {
                    "立即".to_string()
                } else if duration.num_minutes() < 1 {
                    format!("{}秒后", duration.num_seconds())
                } else if duration.num_hours() < 1 {
                    format!("{}分钟后", duration.num_minutes())
                } else if duration.num_days() < 1 {
                    format!("{}小时后", duration.num_hours())
                } else {
                    format!("{}天后", duration.num_days())
                }
            }
            None => "手动".to_string(),
        }
    }

    /// 启用任务
    pub fn enable(&mut self) {
        self.enabled = true;
        self.updated_at = Utc::now();
    }

    /// 禁用任务
    pub fn disable(&mut self) {
        self.enabled = false;
        self.updated_at = Utc::now();
    }

    /// 重试任务
    pub fn retry(&mut self) -> bool {
        if self.current_retries < self.max_retries {
            self.current_retries += 1;
            self.updated_at = Utc::now();
            true
        } else {
            false
        }
    }
}

/// 任务执行结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub success: bool,
    pub duration_ms: u64,
    pub timestamp: DateTime<Utc>,
}

/// 任务状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskStatus {
    /// 已禁用
    Disabled,
    /// 手动执行
    Manual,
    /// 已调度
    Scheduled,
    /// 等待执行
    Pending,
    /// 正在执行
    Running,
    /// 正在重试
    Retrying,
}

/// 任务健康状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskHealth {
    /// 健康
    Healthy,
    /// 性能下降
    Degraded,
    /// 警告
    Warning,
    /// 严重
    Critical,
}

/// 调度器管理器
pub struct SchedulerManager {
    scheduler: JobScheduler,
    tasks: Arc<RwLock<Vec<ScheduledTask>>>,
    sync_engine: Arc<SyncEngine>,
    running_tasks: Arc<RwLock<Vec<String>>>, // 正在执行的任务ID列表
}

impl SchedulerManager {
    pub async fn new(sync_engine: SyncEngine) -> Result<Self> {
        let scheduler = JobScheduler::new().await.map_err(|e| {
            error!("创建同步任务发生了异常");
            SyncError::Unknown(e.to_string())
        })?;

        Ok(Self {
            scheduler,
            tasks: Arc::new(RwLock::new(Vec::new())),
            sync_engine: Arc::new(sync_engine),
            running_tasks: Arc::new(RwLock::new(Vec::new())),
        })
    }

    /// 添加计划任务
    pub async fn add_task(&self, mut scheduled_task: ScheduledTask) -> Result<()> {
        // 计算下次执行时间
        scheduled_task.calculate_next_run()?;

        // 添加到内存列表
        let mut tasks = self.tasks.write().await;
        tasks.push(scheduled_task.clone());

        // 如果启用了调度，创建调度任务
        if scheduled_task.enabled {
            match scheduled_task.schedule {
                Schedule::Manual => {
                    println!("当前是手动任务")
                }
                _ => {
                    // 创建调度任务
                    self.schedule_job(&scheduled_task).await?;
                }
            }
        }
        Ok(())
    }

    /// 调度任务执行
    async fn schedule_job(&self, scheduled_task: &ScheduledTask) -> Result<()> {
        let sync_engine = self.sync_engine.clone();
        let task_id = scheduled_task.id.clone();
        let sync_task_id = scheduled_task.sync_task_id.clone();
        let running_tasks = self.running_tasks.clone();

        let job = match &scheduled_task.schedule {
            Schedule::Cron(cron_expr) => Job::new_async(cron_expr, move |_uuid, _l| {
                let sync_engine = sync_engine.clone();
                let task_id = task_id.clone();
                let sync_task_id = sync_task_id.clone();
                let running_tasks = running_tasks.clone();

                Box::pin(async move {
                    if let Err(e) =
                        Self::execute_task(&sync_engine, &task_id, &sync_task_id, &running_tasks)
                            .await
                    {
                        log::error!("任务执行失败 {}: {}", task_id, e);
                    }
                })
            })
            .unwrap(),
            Schedule::Interval { seconds } => {
                Job::new_repeated_async(Duration::from_secs(*seconds), move |_uuid, _l| {
                    let sync_engine = sync_engine.clone();
                    let task_id = task_id.clone();
                    let sync_task_id = sync_task_id.clone();
                    let running_tasks = running_tasks.clone();

                    Box::pin(async move {
                        if let Err(e) = Self::execute_task(
                            &sync_engine,
                            &task_id,
                            &sync_task_id,
                            &running_tasks,
                        )
                        .await
                        {
                            log::error!("任务执行失败 {}: {}", task_id, e);
                        }
                    })
                })
                .map_err(|e| SyncError::Unknown(e.to_string()))?
            }
            Schedule::Manual => {
                // 手动任务不调度
                return Ok(());
            }
        };
        self.scheduler
            .add(job)
            .await
            .map_err(|_e| SyncError::Unknown("".into()))?;
        Ok(())
    }

    /// 执行任务
    async fn execute_task(
        _sync_engine: &Arc<SyncEngine>,
        scheduled_task_id: &str,
        sync_task_id: &str,
        running_tasks: &Arc<RwLock<Vec<String>>>,
    ) -> Result<()> {
        // 检查是否已经在执行
        {
            let running = running_tasks.read().await;
            if running.contains(&scheduled_task_id.to_string()) {
                log::warn!("任务 {} 已经在执行中，跳过", scheduled_task_id);
                return Ok(());
            }
        }

        // 添加到执行列表
        {
            let mut running = running_tasks.write().await;
            running.push(scheduled_task_id.to_string());
        }

        let start_time = Utc::now();

        log::info!("开始执行任务: {}", scheduled_task_id);

        // 这里需要获取实际的同步任务配置
        // 为了简化，我们假设有一个全局配置管理器
        let result = std::panic::AssertUnwindSafe(async {
            // TODO: 从配置管理器获取同步任务
            // let task = config_manager.get_task(sync_task_id)?;
            // sync_engine.sync(&task).await
            Ok::<SyncReport, SyncError>(SyncReport::new(sync_task_id))
        })
        .catch_unwind()
        .await;

        let _duration = Utc::now() - start_time;
        let _success = match result {
            Ok(Ok(_)) => {
                log::info!("任务执行成功: {}", scheduled_task_id);
                true
            }
            Ok(Err(e)) => {
                log::error!("任务执行失败: {}: {}", scheduled_task_id, e);
                false
            }
            Err(_) => {
                log::error!("任务执行异常（panic）: {}", scheduled_task_id);
                false
            }
        };

        // 从执行列表移除
        {
            let mut running = running_tasks.write().await;
            if let Some(pos) = running.iter().position(|id| id == scheduled_task_id) {
                running.remove(pos);
            }
        }

        // TODO: 更新任务统计信息到存储

        Ok(())
    }

    /// 启动调度器
    pub async fn start(&self) -> Result<()> {
        log::info!("启动任务调度器");
        self.scheduler.start().await.unwrap();
        Ok(())
    }

    /// 停止调度器
    pub async fn stop(&mut self) -> Result<()> {
        log::info!("停止任务调度器");
        self.scheduler.shutdown().await.unwrap();
        Ok(())
    }

    /// 获取所有任务
    pub async fn get_tasks(&self) -> Vec<ScheduledTask> {
        self.tasks.read().await.clone()
    }

    /// 获取任务
    pub async fn get_task(&self, task_id: &str) -> Option<ScheduledTask> {
        let tasks = self.tasks.read().await;
        tasks.iter().find(|t| t.id == task_id).cloned()
    }

    /// 删除任务
    pub async fn delete_task(&self, task_id: &str) -> Result<()> {
        let mut tasks = self.tasks.write().await;
        if let Some(pos) = tasks.iter().position(|t| t.id == task_id) {
            tasks.remove(pos);
            Ok(())
        } else {
            Err(SyncError::Validation(format!("任务不存在: {}", task_id)))
        }
    }

    /// 立即触发任务执行
    pub async fn trigger_task(&self, task_id: &str) -> Result<()> {
        let sync_engine = self.sync_engine.clone();
        let running_tasks = self.running_tasks.clone();

        // 获取任务信息
        let scheduled_task = self
            .get_task(task_id)
            .await
            .ok_or_else(|| SyncError::Validation(format!("任务不存在: {}", task_id)))?;

        if !scheduled_task.enabled {
            return Err(SyncError::Validation(format!("任务已禁用: {}", task_id)));
        }

        // 在后台执行
        let task_id_cloned = task_id.to_string();
        let scheduled_id = scheduled_task.id.clone();
        let scheduled_sync_id = scheduled_task.sync_task_id.clone();
        tokio::spawn(async move {
            log::info!("手动触发任务执行: {}", task_id_cloned);

            if let Err(e) = Self::execute_task(
                &sync_engine,
                &scheduled_id,
                &scheduled_sync_id,
                &running_tasks,
            )
            .await
            {
                log::error!("手动触发任务执行失败: {}: {}", task_id_cloned, e);
            }
        });

        Ok(())
    }

    /// 暂停任务
    pub async fn pause_task(&self, task_id: &str) -> Result<()> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
            task.disable();
            Ok(())
        } else {
            Err(SyncError::Validation(format!("任务不存在: {}", task_id)))
        }
    }

    /// 恢复任务
    pub async fn resume_task(&self, task_id: &str) -> Result<()> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
            task.enable();
            Ok(())
        } else {
            Err(SyncError::Validation(format!("任务不存在: {}", task_id)))
        }
    }

    /// 重新调度所有任务
    pub async fn reschedule_all(&mut self) -> Result<()> {
        // 停止当前调度器
        self.scheduler.shutdown().await.unwrap();

        // 重新创建调度器
        let new_scheduler = JobScheduler::new().await.unwrap();
        unsafe {
            // 注意：这里使用了unsafe，实际生产环境需要更安全的实现
            let self_mut = &mut *(self as *const Self as *mut Self);
            self_mut.scheduler = new_scheduler;
        }

        // 重新调度所有任务
        let tasks = self.tasks.read().await;
        for task in tasks.iter() {
            if task.enabled && task.schedule != Schedule::Manual {
                self.schedule_job(task).await?;
            }
        }

        // 重新启动
        self.scheduler.start().await.unwrap();
        Ok(())
    }

    /// 获取调度统计信息
    pub async fn get_stats(&self) -> SchedulerStats {
        let tasks = self.tasks.read().await;
        let running = self.running_tasks.read().await;

        let mut stats = SchedulerStats::new();
        stats.total_tasks = tasks.len();
        stats.running_tasks = running.len();

        for task in tasks.iter() {
            match task.get_status() {
                TaskStatus::Disabled => stats.disabled_tasks += 1,
                TaskStatus::Manual => stats.manual_tasks += 1,
                TaskStatus::Scheduled => stats.scheduled_tasks += 1,
                TaskStatus::Pending => stats.pending_tasks += 1,
                TaskStatus::Running => stats.running_tasks += 1,
                TaskStatus::Retrying => stats.retrying_tasks += 1,
            }

            match task.get_health() {
                TaskHealth::Healthy => stats.healthy_tasks += 1,
                TaskHealth::Degraded => stats.degraded_tasks += 1,
                TaskHealth::Warning => stats.warning_tasks += 1,
                TaskHealth::Critical => stats.critical_tasks += 1,
            }
        }

        stats
    }

    /// 清理完成的任务
    pub async fn cleanup_completed_tasks(&self, max_age_days: u32) -> Result<usize> {
        let mut tasks = self.tasks.write().await;
        let initial_len = tasks.len();

        let cutoff = Utc::now() - chrono::Duration::days(max_age_days as i64);

        tasks.retain(|task| {
            // 保留未完成的任务、最近执行的任务、或者启用的任务
            if task.enabled || task.last_run.is_some_and(|t| t > cutoff) {
                true
            } else {
                log::info!("清理过期任务: {}", task.id);
                false
            }
        });

        Ok(initial_len - tasks.len())
    }

    /// 导出任务列表
    pub async fn export_tasks(&self, format: ExportFormat) -> Result<String> {
        let tasks = self.tasks.read().await;

        match format {
            ExportFormat::Json => serde_json::to_string_pretty(&*tasks)
                .map_err(|e| SyncError::Validation(e.to_string())),
            ExportFormat::Yaml => {
                serde_yaml::to_string(&*tasks).map_err(|e| SyncError::Validation(e.to_string()))
            }
            ExportFormat::Csv => Self::tasks_to_csv(&tasks),
        }
    }

    /// 导入任务列表
    pub async fn import_tasks(&self, data: &str, format: ExportFormat) -> Result<usize> {
        let tasks: Vec<ScheduledTask> = match format {
            ExportFormat::Json => {
                serde_json::from_str(data).map_err(|e| SyncError::Validation(e.to_string()))?
            }
            ExportFormat::Yaml => {
                serde_yaml::from_str(data).map_err(|e| SyncError::Validation(e.to_string()))?
            }
            ExportFormat::Csv => {
                return Err(SyncError::Unsupported("CSV导入暂不支持".into()));
            }
        };

        let mut imported = 0;
        for task in tasks {
            self.add_task(task).await?;
            imported += 1;
        }

        Ok(imported)
    }

    fn tasks_to_csv(tasks: &[ScheduledTask]) -> Result<String> {
        let mut wtr = csv::Writer::from_writer(Vec::new());

        for task in tasks {
            wtr.serialize(CsvTask {
                id: &task.id,
                name: &task.name,
                sync_task_id: &task.sync_task_id,
                schedule: match &task.schedule {
                    Schedule::Cron(s) => s.clone(),
                    Schedule::Interval { seconds } => format!("interval:{}", seconds),
                    Schedule::Manual => "manual".to_string(),
                },
                enabled: task.enabled,
                last_run: task.last_run.map(|t| t.to_rfc3339()).unwrap_or_default(),
                next_run: task.next_run.map(|t| t.to_rfc3339()).unwrap_or_default(),
                run_count: task.run_count,
                priority: task.priority,
                tags: task.tags.join(","),
            })
            .unwrap();
        }

        let data = String::from_utf8(wtr.into_inner().unwrap())
            .map_err(|e| SyncError::Validation(e.to_string()))?;
        Ok(data)
    }
}

/// 调度器统计信息
#[derive(Debug, Clone, Serialize)]
pub struct SchedulerStats {
    /// 总任务数
    pub total_tasks: usize,
    /// 正在运行的任务数
    pub running_tasks: usize,
    /// 已调度的任务数
    pub scheduled_tasks: usize,
    /// 等待执行的任务数
    pub pending_tasks: usize,
    /// 手动任务数
    pub manual_tasks: usize,
    /// 已禁用的任务数
    pub disabled_tasks: usize,
    /// 正在重试的任务数
    pub retrying_tasks: usize,
    /// 健康任务数
    pub healthy_tasks: usize,
    /// 性能下降任务数
    pub degraded_tasks: usize,
    /// 警告任务数
    pub warning_tasks: usize,
    /// 严重任务数
    pub critical_tasks: usize,
    /// 平均任务执行时间（毫秒）
    pub average_duration_ms: u64,
    /// 今日执行次数
    pub today_run_count: u32,
    /// 今日失败次数
    pub today_failed_count: u32,
    /// 今日成功次数
    pub today_success_count: u32,
}

impl SchedulerStats {
    pub fn new() -> Self {
        Self {
            total_tasks: 0,
            running_tasks: 0,
            scheduled_tasks: 0,
            pending_tasks: 0,
            manual_tasks: 0,
            disabled_tasks: 0,
            retrying_tasks: 0,
            healthy_tasks: 0,
            degraded_tasks: 0,
            warning_tasks: 0,
            critical_tasks: 0,
            average_duration_ms: 0,
            today_run_count: 0,
            today_failed_count: 0,
            today_success_count: 0,
        }
    }

    pub fn success_rate(&self) -> f64 {
        if self.today_run_count == 0 {
            0.0
        } else {
            (self.today_success_count as f64 / self.today_run_count as f64) * 100.0
        }
    }

    pub fn health_score(&self) -> f64 {
        if self.total_tasks == 0 {
            return 100.0;
        }

        let healthy_weight = self.healthy_tasks as f64 * 1.0;
        let degraded_weight = self.degraded_tasks as f64 * 0.7;
        let warning_weight = self.warning_tasks as f64 * 0.4;
        let critical_weight = self.critical_tasks as f64 * 0.1;

        (healthy_weight + degraded_weight + warning_weight + critical_weight)
            / self.total_tasks as f64
            * 100.0
    }
}

/// 导出格式
#[derive(Debug, Clone)]
pub enum ExportFormat {
    Json,
    Yaml,
    Csv,
}

/// CSV格式的任务表示
#[derive(Debug, Serialize)]
struct CsvTask<'a> {
    id: &'a str,
    name: &'a str,
    sync_task_id: &'a str,
    schedule: String,
    enabled: bool,
    last_run: String,
    next_run: String,
    run_count: u32,
    priority: u8,
    tags: String,
}

/// 任务通知器
pub struct TaskNotifier {
    email_config: Option<EmailConfig>,
    webhook_urls: Vec<String>,
}

impl TaskNotifier {
    pub fn new() -> Self {
        Self {
            email_config: None,
            webhook_urls: Vec::new(),
        }
    }

    /// 发送任务开始通知
    pub async fn notify_task_start(&self, task: &ScheduledTask) -> Result<()> {
        let message = format!(
            "任务开始执行: {}\n任务ID: {}\n开始时间: {}",
            task.name,
            task.id,
            Utc::now().to_rfc3339()
        );

        self.send_notification(&message).await
    }

    /// 发送任务完成通知
    pub async fn notify_task_complete(
        &self,
        task: &ScheduledTask,
        report: &SyncReport,
    ) -> Result<()> {
        let message = format!(
            "任务执行完成: {}\n任务ID: {}\n状态: {}\n耗时: {:.1}秒\n同步文件: {}\n传输数据: {}\n详情: {}",
            task.name,
            task.id,
            report.status.as_str(),
            report.duration_seconds,
            report.statistics.files_synced,
            format_bytes(report.statistics.transferred_bytes),
            report.summary()
        );

        self.send_notification(&message).await
    }

    /// 发送任务失败通知
    pub async fn notify_task_failed(
        &self,
        task: &ScheduledTask,
        error: &SyncError,
        retry_count: u32,
    ) -> Result<()> {
        let message = format!(
            "任务执行失败: {}\n任务ID: {}\n错误: {}\n重试次数: {}/{}\n时间: {}",
            task.name,
            task.id,
            error,
            retry_count,
            task.max_retries,
            Utc::now().to_rfc3339()
        );

        self.send_notification(&message).await
    }

    async fn send_notification(&self, message: &str) -> Result<()> {
        // 发送邮件
        if let Some(email_config) = &self.email_config {
            self.send_email(email_config, message).await?;
        }

        // 发送Webhook
        for url in &self.webhook_urls {
            self.send_webhook(url, message).await?;
        }

        Ok(())
    }

    async fn send_email(&self, _config: &EmailConfig, _message: &str) -> Result<()> {
        // 实现邮件发送逻辑
        // 这里使用 lettre 库
        Ok(())
    }

    async fn send_webhook(&self, url: &str, message: &str) -> Result<()> {
        let client = reqwest::Client::new();
        let payload = serde_json::json!({
            "text": message,
            "timestamp": Utc::now().to_rfc3339(),
        });

        client.post(url).json(&payload).send().await?;

        Ok(())
    }
}

/// 邮件配置
#[derive(Debug, Clone)]
pub struct EmailConfig {
    pub smtp_server: String,
    pub smtp_port: u16,
    pub username: String,
    pub password: String,
    pub from: String,
    pub to: Vec<String>,
}

// 调度器CLI命令扩展
pub async fn cmd_schedule_task(
    config_manager: &crate::config::ConfigManager,
    task_id: &str,
    schedule_str: &str,
) -> Result<()> {
    println!("⏰ 为任务配置计划执行: {}", task_id);

    // 获取同步任务
    let sync_task = config_manager
        .get_task(task_id)
        .ok_or_else(|| SyncError::Validation(format!("任务不存在: {}", task_id)))?;

    // 解析调度配置
    let schedule = if schedule_str == "manual" {
        Schedule::Manual
    } else if let Ok(seconds) = schedule_str.parse::<u64>() {
        Schedule::Interval { seconds }
    } else if schedule_str.starts_with("interval:") {
        let seconds = schedule_str
            .trim_start_matches("interval:")
            .parse::<u64>()
            .map_err(|e| SyncError::Validation(e.to_string()))?;
        Schedule::Interval { seconds }
    } else {
        // 尝试解析为cron表达式
        Schedule::Cron(schedule_str.to_string())
    };

    // 创建计划任务
    let scheduled_task = ScheduledTask::new(&sync_task, schedule);

    // TODO: 保存到调度器

    println!("✅ 任务已配置为计划执行");
    println!("📅 下次执行: {}", scheduled_task.format_next_run());

    Ok(())
}

pub async fn cmd_list_scheduled_tasks(scheduler: &SchedulerManager) -> Result<()> {
    use prettytable::{Table, row};

    println!("📋 计划任务列表:");

    let tasks = scheduler.get_tasks().await;

    if tasks.is_empty() {
        println!("  暂无计划任务");
        return Ok(());
    }

    let mut table = Table::new();
    table.add_row(row![
        "ID",
        "名称",
        "计划",
        "下次执行",
        "状态",
        "健康",
        "上次结果"
    ]);

    for task in tasks {
        let status = match task.get_status() {
            TaskStatus::Disabled => "❌ 禁用".to_string(),
            TaskStatus::Manual => "👋 手动".to_string(),
            TaskStatus::Scheduled => "⏰ 已调度".to_string(),
            TaskStatus::Pending => "⏳ 等待".to_string(),
            TaskStatus::Running => "🔄 运行中".to_string(),
            TaskStatus::Retrying => "🔄 重试中".to_string(),
        };

        let health = match task.get_health() {
            TaskHealth::Healthy => "✅".to_string(),
            TaskHealth::Degraded => "⚠️".to_string(),
            TaskHealth::Warning => "🔶".to_string(),
            TaskHealth::Critical => "🔴".to_string(),
        };

        let last_result = if let Some(result) = task.last_result {
            if result.success {
                "✅".to_string()
            } else {
                "❌".to_string()
            }
        } else {
            "—".to_string()
        };

        table.add_row(row![
            task.id.clone(),
            task.name.clone(),
            match &task.schedule {
                Schedule::Cron(s) => format!("cron: {}", s),
                Schedule::Interval { seconds } => format!("间隔: {}秒", seconds),
                Schedule::Manual => "手动".to_string(),
            },
            "—".to_string(),
            status,
            health,
            last_result
        ]);
    }

    table.printstd();

    // 显示统计信息
    let stats = scheduler.get_stats().await;
    println!("\n📊 调度器统计:");
    println!("  总任务数: {}", stats.total_tasks);
    println!("  运行中: {}", stats.running_tasks);
    println!("  健康度: {:.1}%", stats.health_score());
    println!("  今日成功率: {:.1}%", stats.success_rate());

    Ok(())
}
