# 当前状态（唯一事实源）

- 最后核对：2026-10-21（Asia/Shanghai）
- 仓库：`https://github.com/Barabama/zhfd-checkin`
- 本文件是**当前状态的唯一事实源**。其他文档只描述稳定知识；状态数字以这里为准。

## 1. 代码与发布

| 项目 | 值 |
|---|---|
| 默认分支 | `master` |
| HEAD（发布） | `c60918c`（`v0.3.5` tag） |
| HEAD 与远端 | 与 `origin/master` 同步 |
| Cargo 版本 | `0.3.5` |
| `v0.3.5` tag commit | `c60918c` |
| 最新 GitHub Release | `v0.3.5` |
| Release 资产 | `zhfd-checkin-windows-x64-v0.3.5-c60918c14b28.zip` |

> `v0.3.5` 已发布（PR #3）：Phase 2a（变更型命令确认、run 摘要/`--json`、alias）、Phase 2b（独立 `login --confirm-login`，ADR-0005）、Phase 3a（`commands.rs`/`cli.rs`、`RunPolicy`、GUI 去 clap、设备退出码）、以及 App「是否重启App / 界面劫持」弹窗识别（`app_restart_dialog`，不自动点击）。

> `v0.3.4` 已合并（PR #2）：诊断报告/GUI/`config show`/备份的账号 PII 脱敏、`.gitignore` 补齐、旧备份清理；CLI Phase 1（help、clap 约束、配置懒加载、`config path`、解析测试）。一键登录行为不变；dry-run 登录缺口与 login-only 留待后续版本。

> `v0.3.3` 已合并：默认关闭的登录页「一键登录」（[ADR-0004](./adr/ADR-0004-auto-login-opt-in.md)）、LDPlayer 预设对齐、`page`/`foreground`/`auto_login` 分层。Live 一键登录已在 index 0 验证一次；完整签到 live 与独立审计仍未完成。master 行为相对 v0.3.2 新增了上述特征。

### tag → commit

| tag | commit |
|---|---|
| `v0.1.0` | `87658fe` |
| `v0.2.0` | `5835e9d` |
| `v0.3.0` | `06dfaf4` |
| `v0.3.1` | `a955d0a` |
| `v0.3.2` | `8d3f58e` |
| `v0.3.3` | `de2cc8d` |
| `v0.3.4` | `3437b1f` |
| `v0.3.5` | `c60918c` |

> 2026-10-18 重写过 git 历史以移除 fixture PII。重写前的 SHA（例如 `e135369`、`9a66664`、`c4b3bc4`）在本地已不可解析；历史文档中的这些 SHA 只应标注为 pre-rewrite 引用。

## 2. 本地验证基线

```text
cargo fmt --all -- --check                     ✅
cargo clippy --all-targets --all-features -D warnings  ✅
cargo test --lib                               ✅ 167 passed（v0.3.5；v0.3.2 tag 为 140）
cargo test --all-targets                       ✅ lib 167 + bin 167（同一批测试跑两次）
cargo build --release --bins                   ✅
```

`rust/src/lib.rs` 是 `include!("main.rs")`，因此测试会在 lib 和 bin 两个 target 各跑一次。报告数量时优先用 `cargo test --lib`。

## 3. 验证成熟度

| 能力 | 已实现 | 本地测试 | GitHub CI | 真实设备 | 独立验收 |
|---|---|---|---|---|---|
| CLI dry-run / 三实例隔离 | 是 | 是 | 是 | 是（2026-10-17/18） | 部分 |
| 普通签到 live | 是 | 是 | 是 | 是（2026-10-18 22:12，三实例） | **否** |
| 外勤 live opt-in | 是 | 是 | 是 | 仅 dry-run 观察 | **否** |
| 登录页一键登录（默认关闭） | 是 | 是（153 tests） | 未运行（分支） | 是（2026-10-21 00:23，index 0，900x1600@320；00:58 在 720x1280@320 dry-run 验证预设适配） | **否** |
| Windows 计划任务 | 是 | 字符串级 | 是 | 是（21:54/22:12 触发） | 部分 |
| GUI Setup/Repair/迁移 | 是 | 状态机单元测试 | 构建通过 | 未做 GUI 黑盒 | **否** |

说明：2026-10-18 的三实例 live 证据来自 `0.3.1-90d086a+dirty` 构建（包含 v0.3.2 的修复但当时未提交），不是 `v0.3.2` tag 产物。它可以证明行为，不能替代对 tag 构建的独立验收。

## 4. 审计状态

- 最新独立审计：`docs/audits/V0.3.1_INDEPENDENT_AUDIT_REPORT_2026-10-18.md`（对象 `e135369` / `v0.3.1`；pre-rewrite SHA）。
- 该审计结论是「不构成独立验收通过」，列出 9 个 P1。
- v0.3.2 修复了其中 P1-1/P1-2/P1-3/P1-6/P1-7/P1-8；P1-9 通过 ADR-0002 与文档更新解决。
- **v0.3.2 / v0.3.3 / v0.3.4 / v0.3.5 尚无独立审计报告。**
- v0.3.4 的 PII 脱敏与 CLI Phase 1 未独立审计；AI 审计与 remediation 记录见 [CLI 设计审计](./audits/CLI_DESIGN_AUDIT_2026-10-21.md) 与 [V0.3.3 AI 审计报告](./audits/V0.3.3_AUDIT_REPORT_2026-10-21.md)。
- 2026-10-21 AI 审计：[V0.3.3 AI 审计报告](./audits/V0.3.3_AUDIT_REPORT_2026-10-21.md)（对象 `de2cc8d` / HEAD `27f5649`）。确认：诊断报告导出 `auto_login_account` 明文（P1）、dry-run 不能完成一键登录且无 login-only 通道（P1，设计取舍）、release workflow 仍不绑定 tag/版本（P1-4）；另有 GUI/备份/忽略规则等 P2 PII 路径。**AI 辅助，不构成独立验收。**
- 2026-10-21 CLI 设计审计：[CLI 命令面设计审计](./audits/CLI_DESIGN_AUDIT_2026-10-21.md)。结论：最大嵌套 2 层、结构已扁平，但 help 可发现性、CLI/GUI/状态机耦合、`task create --all-instances` 参数组合仍需改进；列为 P1/P2，未改代码。**AI 辅助，不构成独立验收。**
- v0.3.3 的自动登录、预设 DPI 调整与模块拆分仍未独立审计；现场只完成一次 live 一键登录（900x1600@320）与一次 720x1280@320 dry-run，且该 live 证据早于最终 pre-tap/dialog 改动（见 AI 审计 EVID-01）。
- 未关闭项：
  - **P1-4**：release workflow 不绑定 tag 名 / Cargo 版本 / tag 目标；`workflow_dispatch` 可在任意分支运行。
  - **P2-26**：release 不依赖 CI；Python/Node 门禁不在 release workflow 中；CI 在 Python/Node 之前上传 artifact。
  - 其余 P2/P3 见审计报告；多数为诊断、GUI 状态与兼容性问题。

## 5. 已知运行限制

- 平台仅支持 Windows（LDPlayer + ADB + PowerShell + MSVC）。
- `edition = 2024`，需要 Rust ≥ 1.85；仓库没有 `rust-toolchain.toml`，CI 使用 stable。
- `location_timeout_seconds` 会被静默钳制在 1–120 秒。
- `stable_frames = 0` 会被配置校验拒绝；即使绕过校验，`authorize_click` 也会拒绝 0 帧，而不是“立即点击”。
- 完全关机的 LDPlayer 实例可能不出现在 `ldconsole list2`，`--all-instances` 会报 `LDPlayer 实例不存在`；建议运行前先启动实例。
- `task create --all-instances` 与 `--instance-index`/`--serial` 的组合现在由 clap 在解析层拒绝（exit 2）；`--all-instances` 只运行配置中 enabled 的实例。
- App 若弹出「是否重启App / 当前 App 可能被界面劫持」，运行以 `app_restart_dialog` 停止；程序不点击弹窗，需要人工取消/重启 App 并检查悬浮窗/录屏/辅助功能。
- 自动登录的点击坐标来自 hierarchy bounds，不依赖 Profile 几何；但 live 运行仍要求 Profile 已标定。当前仅 `portrait_900x1600_d320`（原 900x1600 生产实例）与历史 `portrait_900x1600_d240` 已标定；LDPlayer 预设 1080x1920@480、720x1280@320、540x960@240 仍是占位几何，live 会在 `uncalibrated_profile` 停止，dry-run 可探测登录页。
- dry-run 不是离线预览：它会启动 App、导航、读取页面并等待定位，只是不点击签到按钮。

## 6. 如何刷新本文件

```powershell
git status -sb
git log --oneline -3
git rev-list -n1 v0.3.3
git tag --list
cd rust; cargo test --lib
```

发布后请同时更新：本文件的 HEAD/tag/Release/审计状态、`docs/ENGINEERING_AND_RELEASE.md` 的版本段、`docs/README.md` 的“最后核对”说明。
