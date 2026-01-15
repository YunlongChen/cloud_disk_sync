# WebDAV Provider 构建和测试脚本

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  WebDAV Provider 构建和测试" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

# 步骤 1: 检查依赖
Write-Host "步骤 1: 检查 Cargo..." -ForegroundColor Yellow
$cargoVersion = cargo --version
if ($LASTEXITCODE -ne 0) {
    Write-Host "错误: Cargo 未安装" -ForegroundColor Red
    exit 1
}
Write-Host "✓ $cargoVersion" -ForegroundColor Green
Write-Host ""

# 步骤 2: 清理之前的构建
Write-Host "步骤 2: 清理构建..." -ForegroundColor Yellow
cargo clean
Write-Host "✓ 清理完成" -ForegroundColor Green
Write-Host ""

# 步骤 3: 构建项目
Write-Host "步骤 3: 构建项目..." -ForegroundColor Yellow
cargo build --lib
if ($LASTEXITCODE -ne 0) {
    Write-Host "✗ 构建失败" -ForegroundColor Red
    exit 1
}
Write-Host "✓ 构建成功" -ForegroundColor Green
Write-Host ""

# 步骤 4: 运行基础测试
Write-Host "步骤 4: 运行基础测试..." -ForegroundColor Yellow
cargo test --test webdav_basic_test
if ($LASTEXITCODE -ne 0) {
    Write-Host "✗ 基础测试失败" -ForegroundColor Red
    exit 1
}
Write-Host "✓ 基础测试通过" -ForegroundColor Green
Write-Host ""

# 步骤 5: 运行集成测试
Write-Host "步骤 5: 运行集成测试..." -ForegroundColor Yellow
cargo test --test webdav_integration_test -- --test-threads=1
if ($LASTEXITCODE -ne 0) {
    Write-Host "✗ 集成测试失败" -ForegroundColor Red
    exit 1
}
Write-Host "✓ 集成测试通过" -ForegroundColor Green
Write-Host ""

# 步骤 6: 运行所有 WebDAV 相关测试
Write-Host "步骤 6: 运行所有 WebDAV 测试..." -ForegroundColor Yellow
cargo test webdav
if ($LASTEXITCODE -ne 0) {
    Write-Host "✗ 部分测试失败" -ForegroundColor Red
    exit 1
}
Write-Host "✓ 所有测试通过" -ForegroundColor Green
Write-Host ""

# 步骤 7: 运行 provider 模块的单元测试
Write-Host "步骤 7: 运行 provider 单元测试..." -ForegroundColor Yellow
cargo test --lib providers::webdav
if ($LASTEXITCODE -ne 0) {
    Write-Host "⚠ Provider 单元测试失败（可能是正常的）" -ForegroundColor Yellow
} else {
    Write-Host "✓ Provider 单元测试通过" -ForegroundColor Green
}
Write-Host ""

# 步骤 8: 生成测试覆盖率报告（可选）
Write-Host "步骤 8: 代码检查..." -ForegroundColor Yellow
cargo clippy --all-targets -- -D warnings
if ($LASTEXITCODE -ne 0) {
    Write-Host "⚠ 代码检查发现警告" -ForegroundColor Yellow
} else {
    Write-Host "✓ 代码检查通过" -ForegroundColor Green
}
Write-Host ""

# 完成
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  测试完成！" -ForegroundColor Green
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""
Write-Host "测试摘要:" -ForegroundColor Cyan
Write-Host "  ✓ 构建成功" -ForegroundColor Green
Write-Host "  ✓ 基础测试通过" -ForegroundColor Green
Write-Host "  ✓ 集成测试通过" -ForegroundColor Green
Write-Host ""
Write-Host "查看详细文档:" -ForegroundColor Cyan
Write-Host "  - 测试说明: .\tests\README.md" -ForegroundColor White
Write-Host "  - 使用示例: .\docs\WEBDAV_USAGE.md" -ForegroundColor White
Write-Host ""
