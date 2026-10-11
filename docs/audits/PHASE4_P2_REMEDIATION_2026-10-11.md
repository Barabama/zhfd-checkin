# Phase 4 P2 finding remediation implementation

日期：2026-10-11（Asia/Shanghai）  
分支：`feature/phase1-environment-discovery`  
状态：**代码已改，待独立审计 Agent 复核；不代表 P2 已验收关闭。**

## Finding 1 — GUI 配置启动失败不可见

修改 `rust/src/bin/zhfd-checkin-gui.rs`：

- 启动失败时仍写入 EXE 根目录 `gui-startup-error.log`，内容包含 config 路径和详细错误；
- Windows GUI subsystem 弹出原生错误对话框，说明配置不会被自动覆盖，并引导用户备份后修复 TOML 或临时改名配置文件；
- 对话框显示配置路径、诊断日志路径；日志无法写入时明确说明程序目录权限问题；
- 错误对话框关闭后进程以 exit code 1 退出，不再静默报告成功。

独立复核应验证：非法 TOML、缺配置文件写权限、日志文件被占用/只读、MessageDialog 关闭后的进程码和普通启动路径。

## Finding 2 — Cargo/便携包版本与 v0.2.0 目标不一致

修改 `rust/Cargo.toml` 与根 package lock 条目为 `0.2.0`。此举只同步 package/About/便携包命名所读取的版本元数据；**没有创建 `v0.2.0` tag，没有构建发布包，没有 push，也没有发布**。

独立复核应检查 `cargo metadata`、CLI/GUI About、package_release 的文件名与 README 版本一致，并确认 Git tag/release 仍不存在。

## 实现边界

- 未运行 tests、clippy、build、portable smoke、GUI 动态流程或设备操作；
- Phase 4 的设备、多实例、真实 GUI 与 CI/release runner 环境仍未验收；
- 原始 [独立审计报告](./PHASE4_INDEPENDENT_AUDIT_REPORT_2026-10-11.md) 记录的是修复前观察，finding 不应因代码修改自动标为已关闭。
