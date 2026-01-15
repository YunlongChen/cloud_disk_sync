use crate::error::Result;
use crate::plugins::hooks::{HookContext, HookHandler, HookPriority, PluginHook};
use async_trait::async_trait;
// src/plugins/manager.rs
use std::collections::HashMap;

pub struct HookManager {
    handlers: HashMap<String, Vec<Box<dyn HookHandler>>>,
    hooks_by_priority: HashMap<PluginHook, Vec<Box<dyn HookHandler>>>,
}

impl HookManager {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            hooks_by_priority: HashMap::new(),
        }
    }

    pub fn register_handler(&mut self, handler: Box<dyn HookHandler>) {
        let handlers = self
            .handlers
            .entry("all".to_string())
            .or_insert_with(Vec::new);
        handlers.push(handler);
    }

    pub async fn execute_hook(&self, hook: PluginHook, context: &mut HookContext) -> Result<()> {
        if let Some(handlers) = self.handlers.get("all") {
            // 按优先级排序
            let mut sorted_handlers: Vec<_> = handlers
                .iter()
                .map(|h| (h.get_priority(&hook), h.as_ref()))
                .collect();

            sorted_handlers.sort_by(|a, b| b.0.cmp(&a.0)); // 降序，高优先级先执行

            for (_, handler) in sorted_handlers {
                if handler.supports_hook(&hook) {
                    handler.handle_hook(hook.clone(), context).await?;
                }
            }
        }

        Ok(())
    }

    fn all_hook_types() -> Vec<&'static str> {
        vec!["all"]
    }
}

/// 基础插件实现示例
#[derive(Clone)]
pub struct LoggingPlugin {
    name: String,
    version: String,
    enabled: bool,
}

impl LoggingPlugin {
    pub fn new() -> Self {
        Self {
            name: "LoggingPlugin".to_string(),
            version: "1.0.0".to_string(),
            enabled: true,
        }
    }
}

#[async_trait]
impl crate::core::traits::Plugin for LoggingPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn description(&self) -> &str {
        "A plugin that logs all synchronization events"
    }

    async fn initialize(&self) -> Result<()> {
        log::info!("LoggingPlugin initialized");
        Ok(())
    }

    async fn shutdown(&self) -> Result<()> {
        log::info!("LoggingPlugin shutting down");
        Ok(())
    }

    fn hooks(&self) -> Vec<PluginHook> {
        vec![
            PluginHook::PreSync {
                task_id: "*".to_string(),
                priority: HookPriority::Normal,
            },
            PluginHook::PostSync {
                task_id: "*".to_string(),
                priority: HookPriority::Normal,
            },
            PluginHook::OnError {
                priority: HookPriority::Normal,
            },
        ]
    }
}

#[async_trait]
impl HookHandler for LoggingPlugin {
    async fn handle_hook(&self, hook: PluginHook, context: &mut HookContext) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        match &hook {
            PluginHook::PreSync { task_id, .. } => {
                log::info!("Starting sync for task: {}", task_id);
                if let Some(task) = &context.task {
                    log::debug!("Task config: {:?}", task);
                }
            }
            PluginHook::PostSync { task_id, .. } => {
                log::info!("Completed sync for task: {}", task_id);
                if let Some(report) = &context.report {
                    log::debug!("Sync report: {:?}", report.statistics);
                }
            }
            PluginHook::OnError { .. } => {
                if let Some(error) = &context.error {
                    log::error!("Sync error: {}", error);
                }
            }
            _ => {
                // 其他钩子不需要处理
            }
        }

        Ok(())
    }

    fn supports_hook(&self, hook: &PluginHook) -> bool {
        match hook {
            PluginHook::PreSync { .. } => true,
            PluginHook::PostSync { .. } => true,
            PluginHook::OnError { .. } => true,
            _ => false,
        }
    }

    fn get_priority(&self, _hook: &PluginHook) -> HookPriority {
        HookPriority::Normal
    }
}

// 简化：不支持克隆 trait 对象，统一在 \"all\" 分组下执行
