/// WebDAV Provider 日志使用示例
///
/// 展示如何配置和使用 tracing 日志框架
///
/// 运行方式：
/// ```bash
/// # 默认日志级别 (info)
/// cargo run --example webdav_with_logging
///
/// # 显示 debug 日志
/// RUST_LOG=debug cargo run --example webdav_with_logging
///
/// # 只显示 WebDAV provider 的 debug 日志
/// RUST_LOG=cloud_disk_sync::providers::webdav=debug cargo run --example webdav_with_logging
///
/// # 详细的 trace 级别日志
/// RUST_LOG=trace cargo run --example webdav_with_logging
/// ```
use tracing::{Level, info};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() {
    // 初始化日志系统
    // 可以通过环境变量 RUST_LOG 控制日志级别
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::DEBUG) // 默认最高到 DEBUG 级别
        .with_target(true) // 显示模块路径
        .with_thread_ids(true) // 显示线程 ID
        .with_line_number(true) // 显示行号
        .with_file(true) // 显示文件名
        .with_ansi(true) // 彩色输出
        .pretty() // 美化输出
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("设置日志订阅者失败");

    info!("🚀 启动 WebDAV 日志示例");

    // 示例：模拟不同级别的日志
    demo_log_levels().await;

    info!("✅ 示例完成");
}

async fn demo_log_levels() {
    use tracing::{debug, error, trace, warn};

    info!("=== 日志级别示例 ===");

    // ERROR: 严重错误，需要立即关注
    error!("这是一个 ERROR 级别的日志");

    // WARN: 警告，可能需要关注
    warn!("这是一个 WARN 级别的日志");

    // INFO: 重要信息
    info!("这是一个 INFO 级别的日志");

    // DEBUG: 调试信息，帮助开发
    debug!("这是一个 DEBUG 级别的日志");

    // TRACE: 最详细的跟踪信息
    trace!("这是一个 TRACE 级别的日志");

    // 带结构化字段的日志
    let file_size = 1024 * 1024;
    let elapsed_ms = 156;
    info!(
        file_size = %file_size,
        elapsed_ms = %elapsed_ms,
        "文件处理完成"
    );

    // 使用 instrument 宏的函数会自动记录进入和退出
    process_file("test.txt", 2048).await;
}

#[tracing::instrument(skip(size), fields(size = %size))]
async fn process_file(filename: &str, size: u64) {
    use tracing::info;

    info!("开始处理文件");

    // 模拟处理
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    info!("文件处理完成");
}
