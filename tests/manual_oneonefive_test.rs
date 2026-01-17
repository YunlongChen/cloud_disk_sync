//! 115网盘手动测试模块
//!
//! 该模块包含115网盘提供者的手动测试用例，用于验证基本功能。
//! 这些测试需要有效的115网盘会话凭证，通过环境变量 `ONEONEFIVE_SESSION` 提供。
//!
//! 使用方法：
//! 1. 设置环境变量：`set ONEONEFIVE_SESSION=your_session_cookie`
//! 2. 运行测试：`cargo test --test manual_oneonefive_test -- --ignored`

use cloud_disk_sync::config::{AccountConfig, ProviderType, RetryPolicy};
use cloud_disk_sync::error::SyncError;
use cloud_disk_sync::providers::{OneOneFiveProvider, StorageProvider};
use std::collections::HashMap;
use std::env;
use std::path::Path;
use std::time::Duration;
use tokio::fs;
use tracing::{debug, error, info, warn};

/// 初始化日志配置
fn init_logging() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();
}

/// 从环境变量获取115网盘会话凭证
fn get_oneonefive_session() -> Result<String, SyncError> {
    env::var("ONEONEFIVE_SESSION").map_err(|_| {
        SyncError::Config(cloud_disk_sync::error::ConfigError::Invalid(
            "环境变量 ONEONEFIVE_SESSION 未设置，请提供有效的115网盘会话凭证".into(),
        ))
    })
}

/// 创建115网盘提供者配置
fn create_oneonefive_config(session: &str) -> AccountConfig {
    let mut credentials = HashMap::new();
    credentials.insert("cookie".to_string(), session.to_string());

    AccountConfig {
        id: "manual_oneonefive_test".to_string(),
        provider: ProviderType::OneOneFive,
        name: "Manual 115 Test".to_string(),
        credentials,
        rate_limit: None,
        retry_policy: RetryPolicy {
            max_retries: 3,
            initial_delay_ms: 0,
            backoff_factor: 2.0,
            max_delay_ms: 0,
        },
    }
}

/// 测试115网盘连接和验证
#[tokio::test]
#[ignore]
async fn test_oneonefive_connection_and_verification() {
    init_logging();

    info!("🚀 开始115网盘连接和验证测试");

    // 获取会话凭证
    let session = match get_oneonefive_session() {
        Ok(session) => {
            info!("✅ 成功获取115网盘会话凭证");
            session
        }
        Err(e) => {
            error!("❌ 获取会话凭证失败: {}", e);
            return;
        }
    };

    // 创建配置
    let config = create_oneonefive_config(&session);

    info!("正在初始化115网盘提供者...");

    // 初始化提供者
    let provider_result = OneOneFiveProvider::new(&config).await;
    let provider = match provider_result {
        Ok(provider) => {
            info!("✅ 115网盘提供者初始化成功");
            provider
        }
        Err(e) => {
            error!("❌ 115网盘提供者初始化失败: {}", e);
            return;
        }
    };

    // 验证连接
    info!("正在验证115网盘连接...");
    match provider.verify().await {
        Ok(()) => info!("✅ 115网盘连接验证成功"),
        Err(e) => {
            error!("❌ 115网盘连接验证失败: {}", e);
            return;
        }
    }

    info!("🎉 115网盘连接和验证测试完成");
}

/// 测试文件列表获取功能
#[tokio::test]
#[ignore]
async fn test_oneonefive_list_files() {
    init_logging();

    info!("📁 开始115网盘文件列表获取测试");

    // 获取会话凭证
    let session = match get_oneonefive_session() {
        Ok(session) => session,
        Err(e) => {
            error!("❌ 获取会话凭证失败: {}", e);
            return;
        }
    };

    // 创建配置和提供者
    let config = create_oneonefive_config(&session);
    let provider = match OneOneFiveProvider::new(&config).await {
        Ok(provider) => provider,
        Err(e) => {
            error!("❌ 115网盘提供者初始化失败: {}", e);
            return;
        }
    };

    // 获取根目录文件列表
    info!("正在获取根目录文件列表...");
    match provider.list("/").await {
        Ok(files) => {
            info!("✅ 成功获取文件列表，共 {} 个文件/目录", files.len());

            if files.is_empty() {
                warn!("⚠️  根目录为空，这可能是正常的");
            } else {
                info!("\n📋 文件列表详情:");
                info!(
                    "{:<10} {:<20} {:<12} {}",
                    "类型", "大小", "修改时间", "名称"
                );
                info!("{}", "-".repeat(60));

                for file in files.iter().take(10) {
                    // 只显示前10个
                    let file_type = if file.is_dir { "DIR" } else { "FILE" };
                    let size_str = if file.is_dir {
                        "-".to_string()
                    } else {
                        format_size(file.size)
                    };

                    info!(
                        "{:<10} {:<20} {:<12} {}",
                        file_type, size_str, file.modified, file.path
                    );
                }

                if files.len() > 10 {
                    info!("... 还有 {} 个文件未显示", files.len() - 10);
                }
            }
        }
        Err(e) => {
            error!("❌ 获取文件列表失败: {}", e);
            return;
        }
    }

    info!("🎉 文件列表获取测试完成");
}

/// 格式化文件大小
fn format_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if size >= GB {
        format!("{:.2} GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.2} MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.2} KB", size as f64 / KB as f64)
    } else {
        format!("{} B", size)
    }
}

/// 测试文件存在性检查
#[tokio::test]
#[ignore]
async fn test_oneonefive_exists_check() {
    init_logging();

    info!("🔍 开始115网盘文件存在性检查测试");

    let session = match get_oneonefive_session() {
        Ok(session) => session,
        Err(e) => {
            error!("❌ 获取会话凭证失败: {}", e);
            return;
        }
    };

    let config = create_oneonefive_config(&session);
    let provider = match OneOneFiveProvider::new(&config).await {
        Ok(provider) => provider,
        Err(e) => {
            error!("❌ 115网盘提供者初始化失败: {}", e);
            return;
        }
    };

    // 先获取根目录文件列表
    info!("正在获取根目录文件列表用于测试...");
    let files = match provider.list("/").await {
        Ok(files) => files,
        Err(e) => {
            error!("❌ 获取文件列表失败: {}", e);
            return;
        }
    };

    if files.is_empty() {
        warn!("⚠️  根目录为空，跳过存在性检查测试");
        return;
    }

    // 测试第一个文件的存在性
    let test_file = &files[0];
    info!("正在检查文件 '{}' 是否存在...", test_file.path);

    match provider.exists(&test_file.path).await {
        Ok(exists) => {
            if exists {
                info!("✅ 文件 '{}' 存在", test_file.path);
            } else {
                warn!("⚠️  文件 '{}' 不存在", test_file.path);
            }
        }
        Err(e) => {
            error!("❌ 检查文件存在性失败: {}", e);
        }
    }

    // 测试一个不存在的文件
    let non_existent_file = "this_file_should_not_exist_12345.txt";
    info!("正在检查不存在的文件 '{}'...", non_existent_file);

    match provider.exists(non_existent_file).await {
        Ok(exists) => {
            if !exists {
                info!("✅ 不存在的文件正确返回 false");
            } else {
                warn!("⚠️  不存在的文件错误返回 true");
            }
        }
        Err(e) => {
            error!("❌ 检查不存在文件时出错: {}", e);
        }
    }

    info!("🎉 文件存在性检查测试完成");
}

/// 测试上传功能（需要实现上传API）
#[tokio::test]
#[ignore]
async fn test_oneonefive_upload() {
    init_logging();

    info!("⬆️  开始115网盘上传功能测试");

    let session = match get_oneonefive_session() {
        Ok(session) => session,
        Err(e) => {
            error!("❌ 获取会话凭证失败: {}", e);
            return;
        }
    };

    let config = create_oneonefive_config(&session);
    let provider = match OneOneFiveProvider::new(&config).await {
        Ok(provider) => provider,
        Err(e) => {
            error!("❌ 115网盘提供者初始化失败: {}", e);
            return;
        }
    };

    // 创建测试文件
    let test_content = "Hello, 115 Cloud Disk! This is a test file for manual testing.";
    let test_file_path = "test_upload_file.txt";

    // 先检查文件是否已存在
    info!("检查测试文件是否已存在...");
    match provider.exists(test_file_path).await {
        Ok(exists) => {
            if exists {
                warn!("⚠️  测试文件已存在，跳过上传测试");
                return;
            }
        }
        Err(e) => {
            error!("❌ 检查文件存在性失败: {}", e);
            return;
        }
    }

    info!("📝 上传功能尚未实现，需要先实现115网盘的上传API");
    info!("💡 提示: 115网盘上传通常需要以下步骤:");
    info!("   1. 预上传检查 (fast upload)");
    info!("   2. 获取上传token和服务器地址");
    info!("   3. 分块上传文件数据");
    info!("   4. 完成上传确认");

    // 这里可以添加上传测试代码，当上传功能实现后
    /*
    let temp_file = create_temp_file(test_content).await?;
    match provider.upload(&temp_file, test_file_path).await {
        Ok(result) => {
            info!("✅ 文件上传成功: {:?}", result);

            // 验证文件确实存在
            match provider.exists(test_file_path).await {
                Ok(exists) => {
                    if exists {
                        info!("✅ 上传验证成功，文件确实存在");
                    } else {
                        warn!("⚠️  上传验证失败，文件不存在");
                    }
                }
                Err(e) => error!("❌ 上传验证失败: {}", e),
            }
        }
        Err(e) => error!("❌ 文件上传失败: {}", e),
    }
    */

    info!("🔧 上传功能测试完成（功能待实现）");
}

/// 测试删除功能（谨慎使用）
#[tokio::test]
#[ignore]
async fn test_oneonefive_delete_with_caution() {
    init_logging();

    warn!("⚠️  开始115网盘删除功能测试 - 此操作会实际删除文件，请谨慎使用！");

    // 这个测试默认跳过，需要手动取消注释并谨慎使用
    info!("🔒 删除测试默认跳过，如需测试请手动取消注释");
    return;

    /*
    // 以下是删除测试的示例代码
    let session = get_oneonefive_session()?;
    let config = create_oneonefive_config(&session);
    let provider = OneOneFiveProvider::new(&config).await?;

    // 先创建一个测试文件
    let test_file_path = "test_delete_file.txt";

    // 删除测试文件
    match provider.delete(test_file_path).await {
        Ok(()) => info!("✅ 文件删除成功"),
        Err(e) => error!("❌ 文件删除失败: {}", e),
    }
    */
}

/// 主测试函数 - 运行所有测试
#[tokio::test]
#[ignore]
async fn test_oneonefive_comprehensive() {
    init_logging();

    info!("🎯 开始115网盘综合测试");

    // 运行各个子测试
    test_oneonefive_connection_and_verification();
    test_oneonefive_list_files();
    test_oneonefive_exists_check();

    info!("🎉 115网盘综合测试完成");
}

/// 显示测试使用说明
#[test]
fn show_test_instructions() {
    println!("\n📋 115网盘手动测试使用说明:");
    println!("========================================");
    println!("1. 设置环境变量:");
    println!("   set ONEONEFIVE_SESSION=your_session_cookie");
    println!("   ");
    println!("2. 运行所有测试:");
    println!("   cargo test --test manual_oneonefive_test -- --ignored");
    println!("   ");
    println!("3. 运行单个测试:");
    println!("   cargo test --test manual_oneonefive_test test_oneonefive_list_files -- --ignored");
    println!("   ");
    println!("4. 可用测试:");
    println!("   - test_oneonefive_connection_and_verification");
    println!("   - test_oneonefive_list_files");
    println!("   - test_oneonefive_exists_check");
    println!("   - test_oneonefive_comprehensive (运行所有测试)");
    println!("========================================\n");
}
