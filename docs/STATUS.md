# 当前状态（唯一事实源）

- 最后核对：2026-10-20（Asia/Shanghai）
- 仓库：`https://github.com/Barabama/zhfd-checkin`
- 本文件是**当前状态的唯一事实源**。其他文档只描述稳定知识；状态数字以这里为准。

## 1. 代码与发布

| 项目 | 值 |
|---|---|
| 默认分支 | `master` |
| HEAD | `1b1931f`（docs: privacy history rewrite note） |
| HEAD 与远端 | 与 `origin/master` 同步 |
| Cargo 版本 | `0.3.2` |
| `v0.3.2` tag commit | `8d3f58e`（HEAD 比它多 2 个 docs-only 提交） |
| 最新 GitHub Release | `v0.3.2` |
| Release 资产 | `zhfd-checkin-windows-x64-v0.3.2-d63d3b3a38af.zip`（资产名保留打包时的旧 commit；二进制代码与 `8d3f58e` 相同） |

> 未发布分支：`feat/auto-login` 实现了默认关闭的登录页「一键登录」（[ADR-0004](./adr/ADR-0004-auto-login-opt-in.md)），尚未合并、发布或完成现场 live 验证。master 行为仍与 `v0.3.2` 一致。

### tag → commit

| tag | commit |
|---|---|
| `v0.1.0` | `87658fe` |
| `v0.2.0` | `5835e9d` |
| `v0.3.0` | `06dfaf4` |
| `v0.3.1` | `a955d0a` |
| `v0.3.2` | `8d3f58e` |

> 2026-10-18 重写过 git 历史以移除 fixture PII。重写前的 SHA（例如 `e135369`、`9a66664`、`c4b3bc4`）在本地已不可解析；历史文档中的这些 SHA 只应标注为 pre-rewrite 引用。

## 2. 本地验证基线

```text
cargo fmt --all -- --check                     ✅
cargo clippy --all-targets --all-features -D warnings  ✅
cargo test --lib                               ✅ 153 passed（分支；v0.3.2 tag 为 140）
cargo test --all-targets                       ✅ lib 153 + bin 153（同一批测试跑两次）
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
- **v0.3.2 尚无独立审计报告。**
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
- `task create --all-instances` 与 `--instance-index`/`--serial` 的组合在运行时会失败；不要这样创建。
- 自动登录的点击坐标来自 hierarchy bounds，不依赖 Profile 几何；但 live 运行仍要求 Profile 已标定。当前仅 `portrait_900x1600_d320`（原 900x1600 生产实例）与历史 `portrait_900x1600_d240` 已标定；LDPlayer 预设 1080x1920@480、720x1280@320、540x960@240 仍是占位几何，live 会在 `uncalibrated_profile` 停止，dry-run 可探测登录页。
- dry-run 不是离线预览：它会启动 App、导航、读取页面并等待定位，只是不点击签到按钮。

## 6. 如何刷新本文件

```powershell
git status -sb
git log --oneline -3
git rev-list -n1 v0.3.2
git tag --list
cd rust; cargo test --lib
```

发布后请同时更新：本文件的 HEAD/tag/Release/审计状态、`docs/ENGINEERING_AND_RELEASE.md` 的版本段、`docs/README.md` 的“最后核对”说明。
