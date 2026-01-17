mod cli;
mod commands;
mod config;
mod core;
mod encryption;
mod error;
mod models;
mod mount;
mod plugins;
mod providers;
mod report;
mod services;
mod sync;
mod utils;

#[cfg(feature = "mount")]
use crate::cli::MountCommands;
use crate::cli::{AccountCmd, Cli, Commands, TaskCmd};

use crate::commands::{
    account::{
        cmd_account_status, cmd_add_account, cmd_browse_account, cmd_list_accounts,
        cmd_remove_account, cmd_update_account,
    },
    completion::cmd_generate_completion,
    diff::cmd_diff_task,
    info::cmd_info,
    key::cmd_generate_key,
    report::cmd_report,
    run::cmd_run_task,
    task::{cmd_create_task, cmd_list_tasks, cmd_remove_task},
    verify::cmd_verify_integrity,
};

#[cfg(feature = "mount")]
use crate::commands::mount::cmd_mount;

use crate::config::ConfigManager;
use clap::Parser;

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
            no_progress,
        } => {
            cmd_run_task(&config_manager, &task, dry_run, false, no_progress).await?;
        }
        Commands::Report {
            task,
            report_id,
            command,
        } => {
            cmd_report(&task, report_id.as_deref(), &command, false).await?;
        }
        Commands::Accounts(cmd) => match cmd {
            AccountCmd::Create {
                name_or_id,
                name,
                provider,
                token,
            } => {
                let account_name = name_or_id
                    .or(name)
                    .ok_or("必须提供账户名称 (使用 --name 或直接提供名称)")?;
                let provider_val = provider.unwrap_or_default();
                cmd_add_account(&mut config_manager, account_name, provider_val, token).await?;
            }
            AccountCmd::List => {
                cmd_list_accounts(&config_manager)?;
            }
            AccountCmd::Remove {
                id,
                name_or_id,
                force,
            } => {
                let target_id = name_or_id
                    .or(id)
                    .ok_or("必须提供账户ID或名称 (使用 --id 或直接提供名称)")?;
                cmd_remove_account(&mut config_manager, &target_id, force)?;
            }
            AccountCmd::Update {
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
            AccountCmd::Status { id, name_or_id } => {
                let target_id = name_or_id
                    .or(id)
                    .ok_or("必须提供账户ID或名称 (使用 --id 或直接提供名称)")?;
                cmd_account_status(&config_manager, &target_id).await?;
            }
            AccountCmd::Browse {
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
            TaskCmd::Create {
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
            TaskCmd::List => {
                cmd_list_tasks(&config_manager)?;
            }
            TaskCmd::Remove {
                id,
                name_or_id,
                name,
                force,
            } => {
                let target_id = name_or_id
                    .or(id)
                    .or(name)
                    .ok_or("必须提供任务ID或名称 (使用 --id 或直接提供名称)")?;
                cmd_remove_task(&mut config_manager, &target_id, force)?;
            }
        },
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
            cmd_info();
        }
        #[cfg(feature = "mount")]
        Commands::Mount(command) => {
            cmd_mount(command)?;
        }
    }

    Ok(())
}
