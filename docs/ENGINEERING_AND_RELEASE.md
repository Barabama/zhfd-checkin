# 工程构建、版本与发布流程

更新时间：2026-10-26（Asia/Shanghai）

> 当前 HEAD/tag/Release/CI/审计状态以 [STATUS.md](./STATUS.md) 为准；本文件只维护工程流程。

## 代码布局

- `rust/src/main.rs`：CLI 命令分发、运行编排、导航和状态机；
- `rust/src/cli.rs`：clap 定义、help 文案、参数约束（CLI 专用，GUI 不依赖）；
- `rust/src/commands.rs`：与 clap 无关的 typed request/policy（`RunPolicy`/`RunRequest`/`RunSource`），CLI 与 GUI 共用；
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

- Cargo package 元数据当前是 `0.3.5`；
- 已发布 tag：`v0.1.0`（`87658fe`）、`v0.2.0`（`5835e9d`）、`v0.3.0`（`06dfaf4`）、`v0.3.1`（`a955d0a`）、`v0.3.2`（`8d3f58e`）、`v0.3.3`（`de2cc8d`）、`v0.3.4`（`3437b1f`）、`v0.3.5`（`c60918c`）；
- `v0.3.5` 包含 Phase 2a/2b（确认型命令、run 摘要/JSON、独立 login）与 Phase 3a（typed command、cli.rs、GUI 解耦），以及 App「是否重启App」弹窗识别；
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
- `v0.3.4` 的 GitHub Release 资产为
  `zhfd-checkin-windows-x64-v0.3.4-3437b1f257dd.zip`，由 tag 触发的 release workflow 生成；
- `v0.3.5` 的 GitHub Release 资产为
  `zhfd-checkin-windows-x64-v0.3.5-c60918c14b28.zip`，由 tag 触发的 release workflow 生成；
- 详细审计与未完成项见
  [V0.3.1 独立审计报告](./audits/V0.3.1_INDEPENDENT_AUDIT_REPORT_2026-10-18.md)；
- 本地工作区不等于发布，tag/Release 仍须独立验收与用户明确批准。

### 发布门禁（2026-10-26 加固）

`release.yml` 现在把发布绑定到 tag 和 Cargo 版本，并复用 CI 门禁。发布前依次检查：

1. `verify` job 以 reusable workflow 调用 `ci.yml`（`release` job `needs: verify`）：
   fmt / clippy / tests / release build / portable smoke / Python / Node 全部通过后才进入
   `release` job——任一门禁失败即不发布；
2. **release guard**（在构建之前运行，fail-fast）：
   - `GITHUB_REF_TYPE` 必须是 `tag`；
   - tag 名必须匹配 `^vX.Y.Z$`（不接受 `v0.4.0-rc.1`、`v0.3.5.1` 这类后缀）；
   - tag 名必须等于 `v<Cargo 版本>`（即 `v0.3.5` ↔ `rust/Cargo.toml` 的 `0.3.5`）；
   - `refs/tags/<tag>` 必须存在于本次 checkout；
   - tag peel 出的提交（`git rev-parse "<tag>^{commit}"`）必须等于 checkout 的 HEAD
     （`git rev-parse "HEAD^{commit}"`）——用 peel 而不是裸对象 id，所以轻量与附注 tag 都成立；
   - tag 指向的提交必须是远端默认分支的祖先（即不能对未合并的功能分支提交打 tag 发布）；
3. **asset guard**（在打包之后、发布之前运行）：`dist/` 必须存在、恰好有一个 zip，
   且文件名必须是 `zhfd-checkin-windows-x64-v<版本>-<12 位 commit>.zip`；
4. 全部通过后才执行 `softprops/action-gh-release`。

因此 `workflow_dispatch` 只能用于**重跑某个 tag 的发布**：在分支上手点会直接失败，
不再可能从任意分支发出 Release。`permissions` 也改为 job 级最小权限——
reusable 的 `ci.yml` 只有 `contents: read`，只有 `release` job 有 `contents: write`。
打 tag 前仍需先 bump `rust/Cargo.toml`。

`ci.yml` 的 artifact 上传已移到 Python / Node 门禁**之后**，失败的门禁不会再留下
一个看起来已验证的 Windows 包。

### 发布流程已关闭的缺口

- ~~release workflow 不校验 tag 名与 Cargo 版本一致，`workflow_dispatch` 可在任意分支运行~~ → 已修复（release guard）；
- ~~release workflow 不运行 Python / Node 门禁~~ → 已修复（`verify` 复用 `ci.yml`）；
- ~~CI 在 Python / Node 之前上传 artifact~~ → 已修复（上传移到门禁之后）。

### 发布流程仍开放的缺口

- 仍要求打 tag 前 bump `rust/Cargo.toml`；不一致现在会被 release guard 直接拒绝（有意的 fail-closed，不再静默产出旧版本号资产）；
- guard 与 asset 命名规则只在本机被**离线模拟 + 真实 tag 复核**过（见下），尚未在 GitHub runner 上真实运行——下一次 tag 发布会是首次真实执行；
- 发布仍依赖 `windows-latest` runner；未在干净 Windows 镜像上做便携验收。

## 本地构建入口

```powershell
cargo build --release --manifest-path .\rust\Cargo.toml --bins
.\scripts\portable_smoke.ps1 -ExePath .\rust\target\release\zhfd-checkin.exe -FixturePath .\autojs6\fixtures\ready.png
.\scripts\package_release.ps1 -ReleaseDirectory .\dist
```

这些是工程/独立审计使用的命令，不表示实现端每次整理文档都应重复验收。版本 bump、tag 和 Release 应在独立测试端审计通过并由用户明确安排后进行。

## 持续集成与审计证据

`.github/workflows/ci.yml` 负责 Windows fmt/Clippy/tests/release-guard 离线回归/release build/portable smoke/Python/Node；`.github/workflows/release.yml` 以 `v*` tag 发布 GitHub Release，并先把上面的 CI 矩阵作为 reusable workflow 跑通（`release` job `needs: verify`）。最近的本地独立测试矩阵、未验证范围和 P2 closed 证据只以 [审计档案](./audits/) 为准；本地通过不替代 GitHub runner 或设备验收。

### 发布门禁的离线回归（`scripts/simulate_release_guards.ps1`）

两条 guard 是「YAML 里的 PowerShell」，在真正打 tag 之前没有任何东西会执行它们。
该脚本**从 `release.yml` 抽出两条 guard 的 `run:` 块**（不是重新实现一遍），在临时
目录里合成一个 bare `origin` + 每个场景一个 scratch clone，然后跑 15 个场景。因为它
直接消费 workflow 里的脚本，改写 guard 会自动反映到测试里，不会出现「测试与实现脱节」。

```powershell
.\scripts\simulate_release_guards.ps1                  # 15 个场景，全部符合预期才退出 0
.\scripts\simulate_release_guards.ps1 -KeepScratch     # 保留临时仓库以便排查
.\scripts\simulate_release_guards.ps1 -CheckRealTag    # 额外用真实 checkout 跑一次 guard
```

默认运行**不写**真实仓库：所有场景都在临时目录的合成仓库里跑，不 push、不打 tag、
不发布。它只从真实仓库**读**两处：`rust/Cargo.toml`（取版本，用于合成 fixture）和
`release.yml`（取 guard 源码）。

`-CheckRealTag` 是显式 opt-in，因为那一次会把 guard 跑在真实 checkout 上，而 guard 内含
`git fetch origin +refs/heads/<default>:refs/remotes/origin/<default>`——它会写真实
`.git` 的 remote-tracking ref 和 `FETCH_HEAD`。只有 HEAD 正好是已发布 tag 时才有意义
（tag push 后的状态）。

CI 在 `cargo test` 之后、release build 之前执行默认（不带 `-CheckRealTag`）的那一遍，
所以 guard 写坏了会在 PR 上失败，而不是在发布时才发现。CI 里会多打印一行
`skipped  release guard: real-repository check is opt-in (-CheckRealTag)`——这是预期，
不是失败。

### 本地模拟结果（2026-10-26）

测试版本取自 `rust/Cargo.toml`（当前 `0.3.5`），因此版本 bump 后脚本仍然自洽。

release guard（9 个场景）：

| 场景 | 期望 | 实测 |
|---|---|---|
| 轻量 tag，checkout 在该 tag 的提交 | 通过 | PASS（`release_guard=PASS tag=v0.3.5 version=0.3.5 default_branch=master`） |
| **附注** tag，checkout 在该 tag 的提交 | 通过 | PASS（`git rev-parse "<tag>^{commit}"` 对两种 tag 结果一致） |
| `workflow_dispatch` 在分支 ref | 拒绝 | REFUSED（`Refusing to release from a 'branch' ref`） |
| tag 名与 Cargo 版本不一致 | 拒绝 | REFUSED（`Tag name does not match the Cargo version`） |
| tag 带预发布后缀 `-rc.1` | 拒绝 | REFUSED（`Release tags must look like vX.Y.Z`） |
| tag 存在但 checkout 了更新的提交 | 拒绝 | REFUSED（`The checked-out commit is not the tagged commit`） |
| tag 指向默认分支没有的提交 | 拒绝 | REFUSED（`… is not an ancestor of refs/remotes/origin/master`） |
| 名字给了但 tag 不存在 | 拒绝 | REFUSED（`v0.3.5 is not present in this checkout`） |
| 默认分支名在远端不存在 | 拒绝 | REFUSED（`Unable to fetch '<branch>' to verify the tag target`） |

asset guard（6 个场景）：唯一 zip 且名字正确 → PASS；名字里版本错、commit 错、
两个 zip（上一轮残留）、没有 zip、`dist/` 不存在 → 全部 REFUSED。

`-CheckRealTag` 已在 `v0.3.5`（`c60918c`）的 checkout 上跑过一次：16 个场景全过，
`release_guard=PASS tag=v0.3.5 version=0.3.5 commit=c60918c… default_branch=master`——
即这条 guard 会接受当前真实的 tag。默认运行里那一项显示为
`skipped  release guard: real-repository check is opt-in (-CheckRealTag)`。

限制：这只覆盖两条 guard 的**逻辑和 PowerShell 运行语义**，不覆盖
`needs: verify` 的 reusable-workflow 调用、`workflow_call` 触发、
`softprops/action-gh-release` 的真实行为，以及 actions/checkout 在 tag push 上留下的
真实 ref 布局。下一次 tag 发布仍是这些改动的首次真实执行。

UiAutomator2 真实 binary manifest fixture 仅用于 parser，不是目标 App APK，也不可安装；见 [fixture 说明](../rust/src/testdata/README.md)。
