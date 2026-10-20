# CLAUDE.md

Windows 便携式「智汇福大晚点名签到」辅助工具：Rust CLI + GUI，通过 ADB 控制 LDPlayer。默认 dry-run；live 点击必须由用户显式授权并通过全部门禁。

## 先读什么

1. `docs/STATUS.md` —— 当前 HEAD / tag / Release / CI / 审计状态（唯一事实源）。
2. `docs/AI_ONBOARDING.md` —— 代码地图、状态机、配置语义、陷阱与 recipes。
3. `docs/CHECKIN_SAFETY.md` —— 页面路由、点击门禁、成功判定。
4. `docs/adr/` —— 对 Frozen 决策的修订。

不要先读 `PRODUCT_AND_ARCHITECTURE.md` 或审计报告来找“当前状态”；审计是时点证据。

## 红线（默认只读）

未经用户明确批准，**不得**：

- 运行 `run --live --confirm`、`profile sync`、`task create/delete`、`app install`；
- 连接/操作真实模拟器或设备；
- 修改 `config.toml`、`runtime.mode`、`runtime.allow_field_work_live`；
- `git push`、打 tag、创建 GitHub Release；
- 提交 `.env`、密钥、PII、原始 hierarchy、运行截图、config、日志或本机路径。

不得削弱 `guarded_tap` / `authorize_page_action` / `authorize_click`，以及 route/mode/前台/窗口/Profile/连续 Ready/点击前复核等门禁。不得修改 Frozen 决策；必须先写 ADR。

可以默认执行：读代码、`cargo fmt --check`、`cargo clippy`、`cargo test`、`cargo build`、只读 git 命令。

## 当前状态

以 `docs/STATUS.md` 为准。核对时运行：

```powershell
git status -sb
git log --oneline -3
git tag --list
cd rust; cargo test --lib
```

当前基线：Cargo `0.3.3`，最新 tag `v0.3.3`，`cargo test --lib` 153 passed。HEAD 可能比 tag 多 docs-only 提交。

## 环境

- Windows 10/11，MSVC toolchain，stable Rust ≥ 1.85（`edition = 2024`）。
- 无 `rust-toolchain.toml`；CI 使用 stable + `windows-latest`。
- 构建溯源需要 git；`build.rs` 会嵌入 commit 与 dirty 状态。

## 常用命令

```powershell
cd rust
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --lib                 # 153 tests
cargo build --release --bins     # 必须 --bins：CLI 与 GUI 一起
```

只读运行：

```powershell
zhfd-checkin.exe about
zhfd-checkin.exe config show
zhfd-checkin.exe instance list
zhfd-checkin.exe diagnose --instance-index 0
zhfd-checkin.exe run --dry-run --instance-index 0
```

`zhfd-checkin.exe` 无子命令会拉起 GUI；脚本必须显式传子命令。

## 代码地图

| 文件 | 职责 |
|---|---|
| `rust/src/main.rs` | CLI、run 编排、状态机、导航、结果、任务、测试（约 4,824 行） |
| `rust/src/page.rs` | 通用页面模型：PageView/PageNode/NodeBounds、路由识别、单轮 hierarchy 观测 |
| `rust/src/foreground.rs` | 前台包观测与 FR-013 检查（自动登录与状态机共享） |
| `rust/src/auto_login.rs` | 登录页识别、协议复选框、一次一键登录与结果字段（不依赖 Profile 几何） |
| `rust/src/domain.rs` | 页面/业务/状态枚举、点击授权、自动登录策略、结果结构、时间窗口 |
| `rust/src/config.rs` | 配置 schema、data root、原子保存/备份、迁移、恢复 |
| `rust/src/gui.rs` | egui 界面（Setup/Repair、运行、诊断、日志、计划任务） |
| `rust/src/adb.rs` | ADB、截图、hierarchy dump、前台/overlay 观测 |
| `rust/src/ldplayer.rs` | ldconsole 发现与实例控制 |
| `rust/src/logger.rs` | result.json / events.jsonl / summary.json |
| `rust/src/vision.rs` | HSV 按钮分类（gray/locating/ready/success） |
| `rust/src/profile.rs` | LDPlayer 预设目录 + 分辨率/DPI → Profile、按钮几何 |

关键位置（用函数名搜索，行号会漂移）：

```text
guarded_tap            main.rs:2745
authorize_page_action  domain.rs:181
authorize_click        domain.rs:523
run_inner_with_device  main.rs:1995   （高风险大函数）
locate_or_navigate     main.rs:2988   （高风险大函数）
observe_page_route     page.rs
detect_page_route      page.rs
extract_hierarchy_date main.rs:1916
```

`rust/src/lib.rs` 是 `include!("main.rs")`；`cargo test` 会把同一批测试在 lib 与 bin 各跑一次。报告数量用 `cargo test --lib`。

## 不可破坏的不变量

- dry-run 永不点击；live 必须 `--live --confirm`。
- 只有 `Checkin` + 授权模式 + 校准 Profile + 目标前台 + 窗口内 + 连续 Ready + 点击前复核才允许 tap。
- 一次运行最多一次 tap；点击后只等待成功，不再进入 Ready 分支。
- 成功必须有目标包作用域的 `签到成功`/`已签到` 文本 + 当天日期；颜色本身不够。
- 未知页面、证据冲突、overlay、外勤未 opt-in 一律 fail-closed。
- 登录页默认 `login_required`；只有 `allow_auto_login` + `accept_login_terms` + live/confirm 才允许一次一键登录；不输入/保存凭据，一次运行最多一次登录点击。
- 数据根单一；配置写入原子替换 + 备份；Repair 状态禁止普通 save/migrate。
- 成功日期只采信目标 package 节点；冲突时返回 unknown。

## 配置与数据根

优先级：显式 `--config` > `%LOCALAPPDATA%\ZHFDCheckin\data-root.txt` > EXE 便携 `config.toml` > LocalAppData > 新建默认。

- 显式 `--config` 指向不存在的文件会创建默认配置。
- 便携配置存在但损坏时不会回退，必须走 GUI Repair。
- 迁移仅 GUI 入口；库函数 `migrate_to_root` 不写指针。
- `location_timeout_seconds` 被钳制到 1–120 秒。
- `stable_frames = 0` 会被配置校验拒绝；即使绕过，`authorize_click` 也拒绝 0 帧。
- 自动登录相关：`allow_auto_login`、`accept_login_terms` 默认均为 false；`auto_login_account` 为可选期望账号（本机配置，禁止提交）；`auto_login_timeout_seconds` 使用时钳制 5–120 秒。

## 文档规则

- 当前状态只写在 `docs/STATUS.md`；其它文档链接它，不复制数字。
- 现行文档需要“最后核对”日期。
- Frozen 决策的修改先写 `docs/adr/`。
- 审计报告是时点证据，只追加 remediation，不改写历史结论。
- 2026-10-18 重写过 git 历史；旧 SHA 是 pre-rewrite 引用，不要当作可解析提交。
- 不提交 PII；fixtures 必须保持脱敏。

## 完成定义

改代码后至少：

1. `cargo fmt --all -- --check`
2. `cargo clippy --all-targets --all-features -- -D warnings`
3. `cargo test --lib`
4. 触及安全逻辑时补反例测试，并说明哪些门禁仍成立
5. 触及状态/发布时更新 `docs/STATUS.md`

遇到以下情况先停下来问人：需要 live/设备操作、需要改 Frozen 决策、需要 push/tag/release、需要删除数据或重写历史。
