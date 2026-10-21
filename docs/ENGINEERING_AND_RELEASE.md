# 工程构建、版本与发布流程

更新时间：2026-10-21（Asia/Shanghai）

> 当前 HEAD/tag/Release/CI/审计状态以 [STATUS.md](./STATUS.md) 为准；本文件只维护工程流程。

## 代码布局

- `rust/src/main.rs`：CLI 命令、运行编排、导航和状态机；
- `rust/src/page.rs`：通用页面模型、路由识别与单轮 hierarchy 观测；
- `rust/src/foreground.rs`：前台包观测与 FR-013 检查；
- `rust/src/auto_login.rs`：登录页选择器与一次一键登录流程；
- `rust/src/domain.rs`：模式、页面/业务/状态契约、结果结构和策略；
- `rust/src/adb.rs`、`rust/src/ldplayer.rs`：设备适配；
- `rust/src/discovery.rs`：依赖候选报告；
- `rust/src/process.rs`：Windows 子进程与 Explorer 启动适配；
- `rust/src/gui.rs`、`rust/src/bin/zhfd-checkin-gui.rs`：egui 界面和 standalone 入口；
- `rust/src/apk.rs`：APK ZIP 与 binary AndroidManifest 解析；
- `rust/src/logger.rs`：日志、结果和诊断报告。

CLI/GUI 共享业务函数，不把 UI 的按钮/输入当作第二套业务策略。

## 版本与发布状态

- Cargo package 元数据当前是 `0.3.4`；
- 已发布 tag：`v0.1.0`（`87658fe`）、`v0.2.0`（`5835e9d`）、`v0.3.0`（`06dfaf4`）、`v0.3.1`（`a955d0a`）、`v0.3.2`（`8d3f58e`）、`v0.3.3`（`de2cc8d`）；`v0.3.4` 已合并（PR #2），待 tag/Release；
- `v0.3.4` 包含账号 PII 脱敏（诊断报告/GUI/`config show`/配置备份）、`.gitignore` 补齐，以及 CLI Phase 1（help、clap 约束、配置懒加载、`config path`、解析测试）；
- `v0.3.3` 包含默认关闭的登录页一键登录、LDPlayer 预设对齐和
  `page`/`foreground`/`auto_login` 模块拆分；
- `v0.3.2` 包含 v0.3.1 之后的计划任务引号、成功日期作用域、数据根目录持久化、
  Repair 保存保护、Setup 门禁和 live 目标快照修复；
- `v0.3.1` 的 GitHub Release 与 CI 已成功运行，发布资产为
  `zhfd-checkin-windows-x64-v0.3.1-9a666641c872.zip`（资产名保留打包时的旧 commit）；
- `v0.3.2` 的 GitHub Release 资产为
  `zhfd-checkin-windows-x64-v0.3.2-d63d3b3a38af.zip`；两版资产只含 EXE 与 README；
- `v0.3.3` 的 GitHub Release 资产为
  `zhfd-checkin-windows-x64-v0.3.3-de2cc8df9085.zip`，由 tag 触发的 release workflow 生成；
- `v0.3.4` 的 Release 资产待 tag 触发 release workflow 后生成；
- 详细审计与未完成项见
  [V0.3.1 独立审计报告](./audits/V0.3.1_INDEPENDENT_AUDIT_REPORT_2026-10-18.md)；
- 本地工作区不等于发布，tag/Release 仍须独立验收与用户明确批准。

### 发布流程已知缺口

- release workflow 不校验 tag 名与 Cargo 版本一致，`workflow_dispatch` 可在任意分支运行；
- release workflow 不运行 Python / Node 门禁；
- CI 在 Python / Node 之前上传 artifact；后续门禁失败时 artifact 仍可下载；
- 打 tag 前必须 bump `rust/Cargo.toml`，否则资产名会使用旧版本号。

## 本地构建入口

```powershell
cargo build --release --manifest-path .\rust\Cargo.toml --bins
.\scripts\portable_smoke.ps1 -ExePath .\rust\target\release\zhfd-checkin.exe -FixturePath .\autojs6\fixtures\ready.png
.\scripts\package_release.ps1 -ReleaseDirectory .\dist
```

这些是工程/独立审计使用的命令，不表示实现端每次整理文档都应重复验收。版本 bump、tag 和 Release 应在独立测试端审计通过并由用户明确安排后进行。

## 持续集成与审计证据

`.github/workflows/ci.yml` 负责 Windows fmt/Clippy/tests/release build/portable smoke/Python/Node；`.github/workflows/release.yml` 以 `v*` tag 发布 GitHub Release。最近的本地独立测试矩阵、未验证范围和 P2 closed 证据只以 [审计档案](./audits/) 为准；本地通过不替代 GitHub runner 或设备验收。

UiAutomator2 真实 binary manifest fixture 仅用于 parser，不是目标 App APK，也不可安装；见 [fixture 说明](../rust/src/testdata/README.md)。
