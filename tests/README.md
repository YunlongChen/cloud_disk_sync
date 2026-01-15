# WebDAV Provider 集成测试

## 测试设计说明

本测试采用**内存模拟WebDAV服务器**的方式，完全不依赖外部服务，具有以下优势：

### 设计特点

1. **零外部依赖**：使用 `warp` 框架在内存中模拟完整的 WebDAV 服务器
2. **快速可靠**：测试在毫秒级完成，不受网络影响
3. **隔离性好**：每个测试独立启动自己的服务器实例
4. **完整覆盖**：测试所有 WebDAV 核心操作

### 测试架构

```
┌─────────────────────┐
│  Integration Test   │
└──────────┬──────────┘
           │
           ↓
┌─────────────────────┐
│  WebDavProvider     │  (被测试的实现)
└──────────┬──────────┘
           │ HTTP
           ↓
┌─────────────────────┐
│ Mock WebDAV Server  │  (使用 warp 实现)
│  (In-Memory)        │
└─────────────────────┘
```

### 模拟服务器实现

模拟服务器支持以下 WebDAV 方法：

- **PROPFIND**：列出文件和目录
- **PUT**：上传文件
- **GET**：下载文件  
- **DELETE**：删除文件/目录
- **MKCOL**：创建目录

## 测试用例列表

### 1. 基础功能测试 (`webdav_basic_test.rs`)

- ✅ `test_webdav_provider_creation`：测试正确创建 provider
- ✅ `test_webdav_provider_missing_credentials`：测试缺少凭证时的错误处理

### 2. 完整集成测试 (`webdav_integration_test.rs`)

#### 文件操作
- ✅ `test_webdav_upload_and_download`：上传下载基本流程
- ✅ `test_webdav_delete`：文件删除功能
- ✅ `test_webdav_upload_multiple_files`：批量上传
- ✅ `test_webdav_large_file_transfer`：大文件传输（1MB）

#### 目录操作
- ✅ `test_webdav_mkdir_and_list`：创建目录和列表查询
- ✅ `test_webdav_exists`：文件存在性检查

#### 高级功能
- ✅ `test_webdav_concurrent_operations`：并发操作测试
- ✅ `test_webdav_error_handling`：错误处理测试

## 运行测试

### 运行所有 WebDAV 测试
```bash
cargo test webdav
```

### 运行基础测试
```bash
cargo test --test webdav_basic_test
```

### 运行集成测试
```bash
cargo test --test webdav_integration_test
```

### 运行特定测试并显示输出
```bash
cargo test test_webdav_upload_and_download -- --nocapture
```

### 并行运行测试
```bash
cargo test webdav -- --test-threads=4
```

## 测试覆盖的场景

### 正常场景
- ✅ 上传小文件（< 1KB）
- ✅ 上传大文件（1MB）
- ✅ 下载文件并验证内容
- ✅ 创建多级目录
- ✅ 列出目录内容
- ✅ 删除文件和目录
- ✅ 并发上传10个文件

### 异常场景
- ✅ 下载不存在的文件
- ✅ 检查不存在的路径
- ✅ 缺少认证信息
- ✅ 重复创建目录

## 测试数据管理

所有测试使用系统临时目录 (`std::env::temp_dir()`) 存储测试文件，测试结束后自动清理：

```rust
let temp_dir = std::env::temp_dir();
let test_file = temp_dir.join("test.txt");
// ... 测试逻辑
tokio::fs::remove_file(&test_file).await.ok();
```

## 性能基准

在现代硬件上的典型测试性能：

- 单个文件上传/下载：< 10ms
- 并发10个文件操作：< 100ms
- 1MB文件传输：< 50ms
- 完整测试套件：< 1s

## 扩展测试

如需测试真实 WebDAV 服务器，可以：

1. 启动本地 WebDAV 服务器（如 nginx + webdav 模块）
2. 配置环境变量：
   ```bash
   export WEBDAV_TEST_URL=http://localhost:8080/dav
   export WEBDAV_TEST_USER=testuser
   export WEBDAV_TEST_PASS=testpass
   ```
3. 运行真实环境测试：
   ```bash
   cargo test webdav_real -- --ignored
   ```

## 依赖说明

测试依赖（dev-dependencies）：
- `warp = "0.3"`：用于模拟 HTTP 服务器
- `bytes = "1.5"`：字节处理

这些依赖仅在测试时使用，不会影响生产代码。
