# Cloud Disk Sync (云盘同步工具)

[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

一个功能强大的跨平台云存储同步工具，支持多网盘协议、加密同步、增量同步及完整性校验。

## ✨ 核心特性

- **多协议支持**: 支持 WebDAV、阿里云盘、115网盘、夸克网盘等多种存储后端。
- **智能同步**:
  - **增量同步**: 仅传输变更的文件，节省流量和时间。
  - **智能差异分析**: 基于文件大小、修改时间和哈希值快速检测变更。
- **安全可靠**:
  - **端到端加密**: 支持 AES-256-GCM 加密，保障数据隐私。
  - **完整性校验**: 提供文件哈希校验，确保数据传输无损。
- **可视化体验**:
  - **实时进度**: 详细的进度条显示，支持多文件并发传输进度。
  - **同步报告**: 生成详细的同步结果报告（支持 JSON/表格输出）。
- **灵活配置**:
  - **任务管理**: 支持创建多个同步任务，灵活配置源和目标路径。
  - **计划任务**: 支持 Cron 表达式和定时间隔自动同步。
  - **过滤规则**: 支持排除特定文件或目录（如隐藏文件）。

## 🚀 快速开始

### 1. 安装

目前请通过源码编译安装：

```bash
# 克隆仓库
git clone https://github.com/your-username/cloud_disk_sync.git
cd cloud_disk_sync

# 编译并运行
cargo run --release -- --help
```

或者直接安装到本地路径：

```bash
cargo install --path .
```

### 2. 基本使用流程

#### 第一步：添加账户

```bash
# 添加 WebDAV 账户 (交互式引导)
cloud-disk-sync accounts create --name my-webdav --provider webdav

# 添加阿里云盘账户
cloud-disk-sync accounts create --name my-aliyun --provider aliyun
```

#### 第二步：创建同步任务

```bash
# 创建任务 (交互式引导)
cloud-disk-sync tasks create

# 或者通过命令行参数直接创建
cloud-disk-sync tasks create \
  --name "Backup Photos" \
  --source "my-local:/" \
  --target "my-webdav:/Photos" \
  --encrypt
```

#### 第三步：运行同步

```bash
# 查看任务列表获取 Task ID
cloud-disk-sync tasks list

# 运行任务
cloud-disk-sync run --task <TASK_ID>

# 仅检查差异不执行 (Dry Run)
cloud-disk-sync run --task <TASK_ID> --dry-run
```

#### 第四步：查看报告

```bash
# 查看最近的同步报告
cloud-disk-sync report --task <TASK_ID>
```

## 📖 命令参考

以下是 `cloud-disk-sync --help` 的输出，包含了所有可用命令：

```text
Usage: cloud-disk-sync [OPTIONS] <COMMAND>

Commands:
  accounts    账户管理 (创建、列表、更新、删除)
  tasks       任务管理 (创建、列表、删除)
  run         运行同步任务
  report      查看同步报告
  verify      验证数据完整性
  gen-key     生成加密密钥
  diff        查看同步差异预览
  info        显示系统及程序信息
  plugins     查看插件列表
  completion  生成 Shell 补全脚本
  help        打印帮助信息

Options:
  -c, --config <CONFIG>  指定配置文件路径 [默认: ~/.config/cloud-disk-sync/config.yaml]
  -v, --verbose          开启详细日志模式
  -h, --help             打印帮助信息
```

### 常用子命令说明

- **`accounts`**:
  - `create`: 添加新账户
  - `list`: 列出已配置账户
  - `remove`: 删除账户
  - `status`: 检查账户连接状态
  - `browse`: 浏览账户文件列表

- **`tasks`**:
  - `create`: 创建新的同步任务
  - `list`: 查看所有任务
  - `remove`: 删除任务

- **`run`**:
  - `--dry-run`: 模拟运行，仅显示将要变更的文件
  - `--no-progress`: 静默模式，不显示进度条

## 🔒 加密功能

本工具支持将本地文件加密后上传至网盘。

1. **生成密钥**:
   ```bash
   cloud-disk-sync gen-key --name my-secret-key
   ```
2. **创建任务时启用加密**:
   在创建任务时选择开启加密，并指定生成的密钥名称。

## 🛠️ 开发与贡献

欢迎提交 Issue 和 Pull Request！

### 环境要求
- Rust 1.75+
- Windows / Linux / macOS

### 运行测试
```bash
cargo test
```

## 📄 许可证

本项目采用 MIT 许可证。详见 [LICENSE](LICENSE) 文件。
