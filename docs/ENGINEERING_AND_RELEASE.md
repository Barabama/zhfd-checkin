# 工程构建、版本与发布流程

更新时间：2026-10-11（Asia/Shanghai）

## 代码布局

- `rust/src/main.rs`：CLI 命令、运行编排、导航和状态机；
- `rust/src/domain.rs`：模式、页面/业务/状态契约、结果结构和策略；
- `rust/src/adb.rs`、`rust/src/ldplayer.rs`：设备适配；
- `rust/src/discovery.rs`：依赖候选报告；
- `rust/src/process.rs`：Windows 子进程与 Explorer 启动适配；
- `rust/src/gui.rs`、`rust/src/bin/zhfd-checkin-gui.rs`：egui 界面和 standalone 入口；
- `rust/src/apk.rs`：APK ZIP 与 binary AndroidManifest 解析；
- `rust/src/logger.rs`：日志、结果和诊断报告。

CLI/GUI 共享业务函数，不把 UI 的按钮/输入当作第二套业务策略。

## 版本与发布状态

- Cargo package 元数据当前是 `0.3.0`；
- P2 复核后的本地审计包版本和清单见 [Phase 4 审计报告](./audits/PHASE4_INDEPENDENT_AUDIT_REPORT_2026-10-11.md)；
- 当前没有基于 `0.3.0` 的正式发布声明；本地工作区不等于发布；
- 独立审计明确：真实 LDPlayer、多实例设备级联、完整 GUI 视觉验收、GitHub CI/release runner 尚未验证。因此 Phase 4 总体验收尚未通过。

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
