# 新 AI 上手手册

- 最后核对：2026-10-21
- 适用对象：第一次接触本仓库的 AI 编码代理或新维护者
- 当前状态数字以 [STATUS.md](./STATUS.md) 为准；本文件讲稳定的结构与规则。

---

## 1. 这是什么 / 不是什么

**是**：Windows 便携式「智汇福大晚点名签到」辅助工具。Rust 编写，包含 CLI（`zhfd-checkin.exe`）和 GUI（`zhfd-checkin-gui.exe`）。它通过 ADB 控制 LDPlayer，完成 App 前台确认、页面导航、签到状态观测，以及**仅在用户显式授权后**对签到按钮执行一次点击。

**不是**：

- 默认不自动登录，不输入或保存账号/密码/验证码；只有同时开启 `runtime.allow_auto_login` 与 `runtime.accept_login_terms` 的 live 运行，才会在登录页执行一次「一键登录」（App 记住的上次账号，不输入凭据）；
- 不伪造定位、设备身份、IMEI 或账号；
- 不是跨平台产品（仅 Windows）；
- `scripts/checkin.py`、`scripts/probe.py`、`autojs6/` 不是发布产品：它们是历史实验 / Python 行为 oracle / AutoJs6 结构检查，不进入发布包。

默认模式是 **dry-run**：可以启动 App、导航、读页面、等待定位，但**不会点击签到按钮**。

---

## 2. 阅读顺序与文档权威层级

推荐顺序：

1. [STATUS.md](./STATUS.md) —— 当前 HEAD/tag/Release/CI/审计状态。
2. 本文件 —— 结构、规则、陷阱。
3. [CHECKIN_SAFETY.md](./CHECKIN_SAFETY.md) —— 页面路由、点击门禁、成功判定。
4. [adr/](./adr/) —— 对冻结决策的修订。
5. 需要具体操作时：[QUICK_START.md](./QUICK_START.md)、[OPERATIONS.md](./OPERATIONS.md)。
6. 历史背景：[audits/](./audits/)、[archive/2026-10-11/](./archive/2026-10-11/)。

权威层级（冲突时按此顺序）：

| 层级 | 文档 | 规则 |
|---|---|---|
| 1 | `docs/STATUS.md` | 当前状态唯一事实源 |
| 2 | 现行文档（本文件、QUICK_START、OPERATIONS、CHECKIN_SAFETY、ENGINEERING_AND_RELEASE） | 可更新，需写“最后核对” |
| 3 | `docs/adr/` | 对 Frozen 决策的正式修订 |
| 4 | `V0.3.0_DECISIONS.md`、`V0.3.0_FIRST_RUN_AND_RECOVERY_REQUIREMENTS.md` | **Frozen**；修改必须先写 ADR |
| 5 | `docs/audits/*` | **时点证据**，不是当前状态；只能追加 remediation，不能改写历史结论 |
| 6 | `docs/archive/` | 历史原件，不再维护 |

---

## 3. 10 分钟只读定位

```powershell
git status -sb
git log --oneline -5
git tag --list
git rev-list -n1 v0.3.4
cd rust
cargo test --lib          # 预期 161 passed（v0.3.2 tag 为 140）
```

在看代码之前，先确认你不在一个脏工作区、没有待发布的本地改动。

---

## 4. 安全模型与 AI 红线

### 4.1 点击门禁（不可削弱）

一次 live 点击必须同时满足：

1. `route == PageRoute::Checkin`；
2. `checkin_mode` 是 `Ordinary`，或 `FieldWork` 且 `runtime.allow_field_work_live = true`；
3. `mode == Live` 且命令行显式 `--live --confirm`；
4. Profile 已标定（`calibrated = true`）；
5. 目标 App 在前台；
6. 在配置的签到时间窗口内；
7. 视觉状态连续 `stable_frames` 帧为 `Ready`；
8. 点击前重新读取 hierarchy、前台包名和新截图，仍确认 `Checkin + 授权模式 + Ready`。

调用链：

```text
run_inner_with_device (main.rs:1975)
  └─ guarded_tap (main.rs:2709)
       ├─ domain::authorize_page_action (domain.rs:181)
       └─ domain::authorize_click (domain.rs:409)
```

### 4.2 fail-closed 规则

以下页面**永远不会**授权点击：`Login`、`Home`、`Services`、`Unknown`、`RouteCandidateUnconfirmed`、`RouteEvidenceConflict`、`StalePageRefreshFailed`、`StalePageRefreshUnconfirmed`、`AppLeftForeground`。

外部窗口（overlay）会阻止导航和点击；前台查询失败与“前台是别的 App”是不同诊断。

### 4.3 登录页（默认拒绝，可选一键登录）

`Login` 默认以 `login_required` 停止。只有同时满足以下条件，live 运行才会执行**一次**一键登录：

1. `runtime.allow_auto_login = true`；
2. `runtime.accept_login_terms = true`（单独授权代勾选《用户协议》《隐私协议》）；
3. `--live --confirm`、目标 App 前台、无外部窗口；
4. 页面来自目标包，且「一键登录」节点唯一；
5. 找到同意复选框，并**视觉确认**勾选成功；
6. 配置了 `runtime.auto_login_account` 时，页面显示的“上次使用过的账号”必须精确匹配。

不输入账号/密码/验证码，不保存凭据，不处理「其他账户登录」；一次运行最多一次登录点击，失败不重试。dry-run 只输出 `auto_login_ready`，永不点击。实现见 [auto_login.rs](../rust/src/auto_login.rs) 的 `auto_login_if_required` / `perform_one_click_login` 与 [domain.rs](../rust/src/domain.rs) 的 `authorize_auto_login`，决策记录见 [ADR-0004](./adr/ADR-0004-auto-login-opt-in.md)。

### 4.4 成功判定

`success = true` 必须同时具备：

- 绿色成功视觉；
- 目标包作用域内的 `签到成功` / `已签到` 文本；
- 可解析的成功日期；
- 日期等于配置时区（默认 `Asia/Shanghai`）的今天。

`dry_run_ready = true` / `field_work_ready = true` **不是**签到成功。`auto_login_succeeded = true` 只表示已离开登录页，也不等于签到成功；`clicked` 仍只表示签到按钮被点击。

### 4.5 AI 默认只读

未经用户明确批准，AI **不得**：

- 运行 `run --live --confirm`、`profile sync`、`task create/delete`、`app install`；
- 连接或操作真实模拟器/设备；
- 修改 `config.toml`、`runtime.mode`、`runtime.allow_field_work_live`；
- `git push`、打 tag、创建 Release；
- 提交 `.env`、密钥、PII、原始 hierarchy、运行截图、config、日志或本机路径。

可以默认执行：读代码、`cargo fmt --check`、`cargo clippy`、`cargo test`、`cargo build`、只读的 `git` 命令。

---

## 5. 仓库地图

```text
rust/
  Cargo.toml / build.rs
  src/
    main.rs        CLI 定义、运行编排、状态机、导航、结果、任务、测试（约 4,824 行）
    page.rs        通用页面模型：PageView/PageNode/NodeBounds、路由识别、单轮观测
    foreground.rs  前台包观测与 FR-013 检查（状态机与自动登录共享）
    auto_login.rs  登录页选择器、协议复选框、一次一键登录（不读 Profile 几何）
    domain.rs      页面/业务/状态枚举、点击授权、自动登录策略、结果结构、时间窗口
    config.rs      配置 schema、data root 选择、原子保存/备份、迁移、恢复
    gui.rs         egui 界面（Setup/Repair、运行、诊断、实例、日志、计划任务）
    bin/zhfd-checkin-gui.rs   无控制台 GUI 入口
    adb.rs         ADB 设备、截图、hierarchy dump、前台/包名、overlay 观测
    ldplayer.rs    ldconsole 发现、实例列表、启动/停止
    discovery.rs   依赖候选与来源报告
    process.rs     Windows 子进程 / Explorer（无窗口）
    apk.rs         APK ZIP / binary AndroidManifest 解析
    vision.rs      HSV 按钮分类（gray/locating/ready/success）
    profile.rs      LDPlayer 预设表 + 分辨率/DPI → Profile、按钮几何
    logger.rs      result.json / events.jsonl / summary.json / 诊断报告
    lib.rs         `include!("main.rs")`（lib target 复用 CLI 实现）
  src/testdata/    仅用于 APK parser 的 fixture，不可安装
autojs6/           AutoJs6 实验工程 + 视觉 fixture（PNG 被 Rust 测试嵌入）
scripts/           Python oracle / 打包 / smoke / 图片分析
docs/              文档（入口：docs/README.md）
.github/workflows/ CI 与 Release
```

---

## 6. 架构与主流程

CLI 与 GUI **共享** `main.rs` 的业务函数：`run`、`diagnose`、`task_command`、`sync_profile`、`install_apk`、`dump_ui_hierarchy`、`list_instance_summaries`。GUI 不复制状态机。

一次 `run` 的流程：

```text
1. 解析模式（--live/--dry-run，否则 runtime.mode）
2. live 必须 --confirm
3. RunLog 目录 + RunResult(schema_version=3)
4. --all-instances：shared preflight → 逐实例 run → batch summary
5. 单实例：
   device_setup → profile 检测 → 启动 App → 等待前台
   → auto_login（仅 allow_auto_login；登录页才生效）
   → locate_or_navigate（stale 页刷新 / 导航 / 观测）
   → 外层模式分支（field work 或普通）
   → 轮询：重读 hierarchy + 前台 + 截图 + 视觉状态
   → Ready + 稳定帧 → dry-run 结束 / live 点击门禁
   → 点击后只等待成功确认，不再点击
6. finalize_run：status/error_code/artifact_paths，写 result.json 与 finish 事件
```

证据目录：

```text
<data-root>/logs/<timestamp>_<pid>/
  result.json
  events.jsonl
  screenshots/state_*.png, navigation_result.png
<data-root>/logs/batch-<id>/summary.json
```

---

## 7. `main.rs` / `auto_login.rs` 导览

按行号分区（数值会随改动漂移，用函数名搜索）：

| 区域 | 起始 | 内容 |
|---|---|---|
| CLI 定义 | `struct Cli` main.rs:60 | clap 子命令与参数 |
| CLI 入口/配置加载 | `run_cli` main.rs:216 | `--config`、about/gui 特例、损坏配置处理 |
| 命令实现 | main.rs:466-1230 | instance/diagnose/config/profile/app/task/ui/vision |
| 单次运行 | `run` main.rs:1326 | 模式解析、RunResult、finalize |
| 批量运行 | `run_all_instances` main.rs:1430 | preflight、逐实例、summary |
| 运行状态机 | `run_inner_with_device` main.rs:1995 | 导航后的轮询/点击/成功确认；前台确认后调用 auto-login |
| 自动登录 | `auto_login_if_required` / `perform_one_click_login` auto_login.rs | 登录页选择器、协议复选框、一次点击、超时轮询 |
| 路由与模式 | `detect_page_route` / `detect_checkin_mode` page.rs | 页面身份 |
| 导航 | `locate_or_navigate` main.rs:2988 | stale 刷新、入口点击、下滑、回退 |
| 页面观测 | `observe_page_route` page.rs | dump 状态、overlay、route |
| 前台检查 | `log_foreground_snapshot` / `foreground_check` foreground.rs | FR-013 前台包观测与分类 |
| 点击门禁 | `guarded_tap` main.rs:2745 | route/mode + click policy |
| 日期解析 | `extract_hierarchy_date` main.rs:1916 | 成功日期与冲突规则 |
| 测试 | main.rs / page.rs / auto_login.rs 测试模块 | 约 161 个测试 |

**高风险区域**：`run_inner_with_device` 与 `locate_or_navigate`。不要盲格式化、移动或重排；先读对应测试，再改。

对应策略在 `domain.rs`：

- `authorize_page_action` domain.rs:181
- `authorize_click` domain.rs:409
- `configured_date` domain.rs:316、`in_configured_window` domain.rs:331

---

## 8. 构建、测试、CI、发布

### 8.1 环境前提

- Windows 10/11；
- Rust stable ≥ 1.85（`edition = 2024`；无 `rust-toolchain.toml`）；
- MSVC toolchain；
- PowerShell（打包/CI）；
- 构建溯源需要 git 与仓库元数据。

### 8.2 本地检查顺序

```powershell
cd rust
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --lib
cargo build --release --bins
cd ..
pwsh -NoProfile -File scripts/portable_smoke.ps1 `
  -ExePath "$PWD/rust/target/release/zhfd-checkin.exe" `
  -FixturePath "$PWD/autojs6/fixtures/ready.png"
```

`cargo test --lib` = 161 passed。`cargo test --all-targets` 会在 lib 与 bin 各跑一次。

### 8.3 构建溯源

`build.rs` 把 `ZHFD_GIT_COMMIT` 与 `ZHFD_GIT_DIRTY` 编进二进制。`about` 输出：

```text
Git commit: <12 位 commit>
Build id: <Cargo version>-<commit>[+dirty|+unknown]
工作区状态: clean | dirty | unknown
```

CI、release workflow、`package_release.ps1` 都拒绝 `+dirty` / `+unknown`。发布前必须在干净树重新构建。

### 8.4 CI / Release

- `.github/workflows/ci.yml`：push/PR → fmt、clippy、test、release build、portable smoke、provenance、打包 artifact、Python、Node。
- `.github/workflows/release.yml`：`v*` tag push（或手动）→ 同上 Rust 门禁 → 打包 → 发布 GitHub Release 与 `dist/*.zip`。
- **已知缺口**：release workflow 不跑 Python/Node；不校验 tag 名与 Cargo 版本一致；`workflow_dispatch` 可在任意分支运行。发布前先确认 CI 在**同一个 commit** 上为绿。

### 8.5 发布资产

只包含 `zhfd-checkin.exe`、`zhfd-checkin-gui.exe`、`README.txt`。不得包含 config、日志、截图、账号、设备状态。

---

## 9. 配置与 data root 手册

配置 schema 见 `rust/src/config.rs` 内嵌默认值。关键语义：

### 9.1 路径优先级（无 `--config`）

1. `%LOCALAPPDATA%\ZHFDCheckin\data-root.txt` 记录的迁移目标；
2. EXE 同目录 `config.toml`；
3. `%LOCALAPPDATA%\ZHFDCheckin\config.toml`；
4. EXE 目录（可写）新建默认配置；
5. LocalAppData 新建默认配置。

显式 `--config <PATH>` 永远最高优先；其父目录就是 data root。

### 9.2 它会做什么 / 不会做什么

| 行为 | 说明 |
|---|---|
| 显式 `--config` 指向不存在的文件 | **会创建**默认配置（不会报错） |
| 便携配置存在但损坏 | **不会**回退到 LocalAppData；报错并指向 GUI Repair |
| Repair 状态下普通 `save()` | 拒绝写入；必须用 `save_candidate` 显式修复 |
| 迁移 | **仅 GUI**；库函数 `migrate_to_root` 不写指针；GUI 迁移成功后写指针 |
| 指针缺失/相对/目标不存在 | 忽略，不阻塞启动 |
| 配置写入 | 同目录临时文件 + 原子替换 + 时间戳备份，保留最近 5 份 |
| `captures/` | 会创建，但 Rust 代码当前不写入 |
| `backups/` | 配置备份目录 |

### 9.3 自动登录配置

| 键 | 默认 | 作用 |
|---|---|---|
| `runtime.allow_auto_login` | `false` | 允许 live 运行在登录页执行一次一键登录 |
| `runtime.accept_login_terms` | `false` | 单独授权程序代勾选《用户协议》《隐私协议》 |
| `runtime.auto_login_account` | `""` | 可选期望账号；非空时登录前必须精确匹配“上次使用过的账号” |
| `runtime.auto_login_timeout_seconds` | `30` | 点击后等待离开登录页的秒数，使用时钳制 5–120 |

- 两开关必须同时为 true；`allow_auto_login` 不隐含协议同意。
- 不输入、不保存账号密码；只支持一键登录，不处理「其他账户登录」。
- `auto_login_account` 是本机 PII，config 已在 `.gitignore` 中；不要提交、不要写进文档或日志。
- 配置备份（`backups/config.toml.bak.*`）会把 `auto_login_account` 与 `bark_url` 写成 `<redacted>`；从备份恢复后需要重新填写账号。`config show` 与诊断报告默认脱敏，GUI 输入框默认掩码。
- 登录页截图可能包含账号，日志目录不得提交。

### 9.4 LDPlayer 预设与 Profile

`profile::LDPLAYER_PRESETS` 是 LDPlayer 14 UI 的 8 个预设（2026-10-21 截图）：

| 方向 | 预设 |
|---|---|
| 横屏 | 1920x1080@280、1600x900@240、1280x720@240、960x540@160 |
| 竖屏 | 1080x1920@480、900x1600@320、720x1280@320、540x960@240 |

- `PROFILES` 必须覆盖每个预设；另保留历史条目 `portrait_900x1600_d240`（已标定，旧安装使用，当前 UI 不再提供，`about`/GUI 标为 legacy）。
- DPI 是 `find_profile` 精确匹配的一部分；LDPlayer 改 DPI 后必须重新检测，否则 `unknown_profile`。
- 已标定：`portrait_900x1600_d320`（原 900x1600 生产实例）与历史 `portrait_900x1600_d240`。改过 DPI 的 1080x1920@480、720x1280@320、540x960@240 仍是 `calibrated=false`，几何比例是旧 DPI 的占位值；live 会在 `uncalibrated_profile` 停止。
- 自动登录的点击坐标来自 hierarchy bounds，**不读 Profile 几何**，因此本身适配所有预设；但 live 自动登录仍受 Profile 标定门禁约束。dry-run 可以在未标定预设上探测登录页并写 `auto_login_ready`。

---

## 10. CLI / GUI 陷阱表

| 陷阱 | 正确做法 |
|---|---|
| 无子命令运行 `zhfd-checkin.exe` | 会拉起 GUI；自动化脚本必须显式传子命令 |
| `run` 的 stdout | 不是纯 JSON；解析 `<data-root>/logs/.../result.json` |
| `stable_frames = 0` | 配置校验会拒绝；即使绕过，`authorize_click` 也拒绝 0 帧 |
| `location_timeout_seconds` | 实际被钳制到 1–120 秒 |
| `task create --all-instances --serial X` / `--instance-index N` | clap 解析层拒绝（exit 2）；`--all-instances` 不能与实例选择器组合 |
| `task query/delete`、`vision`、`profile list` | 不再加载配置；损坏配置下仍可用（`task create` 仍需要配置） |
| `run --live` / `task create --live` / `profile sync --id X` | clap 要求显式 `--confirm`（exit 2）；live 缺少 confirm 的运行期检查仍保留 |
| `--confirm` | 只能与 `--live` 一起用；与 `--dry-run` 冲突（clap 层拒绝） |
| GUI live 确认 | 只快照 path/root/index/all-instances，不比较配置内容 |
| dry-run | 仍会启动 App、导航、等待定位 |
| `ldplayer_reported_port` | 不是 ADB serial；serial 用 `127.0.0.1:{5555+2*index}` |
| `--all-instances` 标签 | 实际是“配置中 enabled 的实例”，不是“当前运行中的实例” |
| `allow_auto_login = true` 但 `accept_login_terms = false` | 自动登录拒绝执行（`auto_login_terms_not_accepted`）；两个开关相互独立 |
| 自动登录的 dry-run | 只输出 `auto_login_ready`，复选框和登录按钮都不会被点击 |
| 自动登录重试 | **没有**重试；一次运行最多一次登录点击，失败需人工介入 |
| `auto_login_account` | 是本机 PII；只写 config，不要写进日志、文档或提交 |
| LDPlayer 改过分辨率/DPI | `find_profile` 精确匹配 DPI；必须重新 `profile detect`，否则 `unknown_profile`；旧 DPI（1080x1920@280、720x1280@240、540x960@160）已不再是预设 |

---

## 11. 结果与错误契约

### 11.1 退出码

| 值 | 含义 |
|---|---|
| 0 | 成功（含 dry-run ready） |
| 1 | 业务失败 |
| 2 | 设备失败（注意：clap 用法错误也退出 2） |
| 3 | 视觉/页面失败 |
| 4 | 配置/未分类错误（任何逃逸的 `anyhow` 错误） |
| 5 | 超时 |

### 11.2 `result.json`

- `schema_version = 3`；
- 关键字段：`status`、`error_code`、`error_stage`、`recovery_action`、`route`/`page_route`、`checkin_mode`、`checkin_state`、`success_date`、`clicked`、`success`、`dry_run_ready`、`field_work_ready`、`auto_login_attempted`、`auto_login_ready`、`auto_login_succeeded`、`auto_login_method`、`auto_login_account_configured`、`auto_login_account_matched`、`auto_login_tap_count`、`artifact_paths`、`page_refresh_attempted/confirmed`；
- 单实例结果没有 `exit_code`；`exit_code` 只在 batch `summary.json` 的 `instances[]` 中；
- `finalize_run` 会把 `running` 收敛为 `success`/`failed`，并保留 `blocked`。

### 11.3 事件与 artifact

- `events.jsonl`：结构化事件（页面观测、route、tap、refresh、blocked 等）；
- 可预览/导出的 artifact 仅限 `result.json`、`summary.json`、`sync.json`、`events.jsonl`；
- 诊断报告会清除 `bark_url`，但分享前仍需人工检查本机路径与截图。

---

## 12. 隐私与历史重写

- 2026-10-18 重写了 git 历史，移除了 fixture 中的真实姓名、班级、定位地址。
- 旧 SHA 在本地已不可解析；历史文档中的旧 SHA 只应标注为 pre-rewrite。
- GitHub 服务端可能仍可按旧 SHA 访问旧提交；force push 不等于服务端 purge。
- 历史 Release 资产名嵌的是打包时的旧 commit SHA；这是预期现象。
- 不得提交：`.env`、密钥、PII、原始 hierarchy、运行截图、config、日志、本机路径。
- `autojs6/fixtures/*.png` 已脱敏，且被 Rust 测试与 portable smoke 使用；修改它们必须同时跑 `cargo test --lib` 和 CI 的 Python/Node 门禁。
- `autojs6/fixtures/fixtures.json` 指向未入库的 `data/images/*`，这是预期的；不要去找源图。

---

## 13. 审计与未关闭项

- 最新审计：[V0.3.1_INDEPENDENT_AUDIT_REPORT_2026-10-18.md](./audits/V0.3.1_INDEPENDENT_AUDIT_REPORT_2026-10-18.md)。
- v0.3.2 修复了其中大部分 P1，但 **v0.3.2 / v0.3.3 / v0.3.4 本身尚未独立审计**；v0.3.3 的自动登录与预设调整同样未审计。
- 仍未关闭：
  - release workflow 不绑定 tag/版本，`workflow_dispatch` 无 ref 限制；
  - release 不依赖 CI，且不跑 Python/Node；
  - GUI 黑盒、低分辨率/高 DPI、迁移中断、真实外勤 live 尚未验收。
- 不要把 commit message 里的 “close P1” 当作独立验收通过。

---

## 14. 常见任务 recipes

### 只读诊断

```powershell
zhfd-checkin.exe about
zhfd-checkin.exe config path
zhfd-checkin.exe config show
zhfd-checkin.exe instance list
zhfd-checkin.exe diagnose --instance-index 0
zhfd-checkin.exe run --dry-run --instance-index 0
```

### 修改点击安全逻辑

1. 先读 `guarded_tap` → `authorize_page_action` → `authorize_click`；
2. 先补反例测试，再改实现；
3. 不得为了让测试通过而放宽 route/mode/前台/窗口/Profile/稳定帧；
4. 跑 `cargo test --lib`，并在结论中说明哪些门禁仍然成立。

### 修改配置 schema

1. 新增字段必须 `#[serde(default)]`，保证旧配置可读；
2. 新默认值要写进 `DEFAULT_CONFIG` 与 `Config::default()`；
3. 补旧配置反序列化测试；
4. 若改变 data root 优先级或 Frozen 决策，先写 ADR。

### 修改 GUI

1. 业务逻辑放 `main.rs`/`config.rs`，GUI 只做展示与调用；
2. 后台任务放线程，`try_lock` 轮询；
3. Setup 门禁统一走 `requires_setup`；
4. 改 live 相关必须保留快照校验。

### 准备发布（需人工批准）

1. 干净树、版本 bump、`cargo test --lib`、`cargo build --release --bins`；
2. `portable_smoke.ps1` + `package_release.ps1` 本地通过；
3. 推送并确认 CI 在目标 commit 上为绿；
4. 打 tag、推送 tag、确认 release workflow；
5. 更新 `docs/STATUS.md` 与 `docs/ENGINEERING_AND_RELEASE.md`。

---

## 15. 首日清单

- [ ] 读 `docs/STATUS.md` 与本文；
- [ ] `git status -sb` 确认工作区干净；
- [ ] `cargo test --lib` 确认 161 passed（v0.3.2 tag 为 140）；
- [ ] 找到 `guarded_tap`、`authorize_click`、`run_inner_with_device`；
- [ ] 确认自己没有运行任何 live/设备命令；
- [ ] 明确本次任务是否触及 Frozen 决策（若是，先写 ADR）。

## 16. 术语表

| 术语 | 含义 |
|---|---|
| dry-run | 观察模式：可导航/读页面/等待定位，不点击签到 |
| live | 真实点击模式：必须 `--live --confirm` 且通过全部门禁 |
| route | 页面身份（签到页/首页/登录页等） |
| checkin_mode | 签到业务类型（普通/外勤/未知） |
| checkin_state | 按钮/签到状态（gray/locating/ready/success…） |
| field work | 外勤签到，独立模式，live 需显式 opt-in |
| stale page refresh | 启动时已在签到页，先 BACK 刷新一次 |
| shared preflight | 批量运行前的公共依赖检查 |
| block | 公共依赖失败时，为每个实例保留 blocked 结果 |
| Profile | 分辨率+DPI 对应的按钮几何与标定状态 |
