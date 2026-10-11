# check-in-zhfd Phase 0–4 独立审计报告

- 审计日期：2026-10-11（Asia/Shanghai）
- 工作区：`D:\Documents\Projects\check-in-zhfd`
- 分支：`feature/phase1-environment-discovery`
- HEAD：`4cfff0a6abed6ff83fe6191939b47bb1d6a3bda1`
- 目标：Phase 0–4 改进的独立测试与边界审计；**不是发布验收或 live 设备验收**。

## 1. 基线与工作区保护

审计开始时按要求记录：

```text
git status --short --branch
## feature/phase1-environment-discovery
 M docs/RUST_MIGRATION.md
 M rust/src/adb.rs
 M rust/src/apk.rs
 M rust/src/domain.rs
 M rust/src/gui.rs
 M rust/src/ldplayer.rs
 M rust/src/logger.rs
 M rust/src/main.rs
?? docs/PHASE0_SYSTEM_CONTRACT_2026-10-09.md
?? docs/PHASE4_ENGINEERING_NOTES_2026-10-10.md
?? docs/PHASE4_INDEPENDENT_AUDIT_PROMPT_2026-10-10.md
?? docs/PROJECT_IMPROVEMENT_BACKLOG_2026-10-09.md
?? rust/src/discovery.rs
?? rust/src/process.rs
?? rust/src/testdata/

git diff --stat
 docs/RUST_MIGRATION.md | 10 +
 rust/src/adb.rs        | 205 ++++++++--
 rust/src/apk.rs        | 615 +++++++++++++++++++----------
 rust/src/domain.rs     | 158 +++++++-
 rust/src/gui.rs        | 637 ++++++++++++++++++++++++++----
 rust/src/ldplayer.rs   | 327 ++++++++++++---
 rust/src/logger.rs     |  91 +++++-
 rust/src/main.rs       | 1030 +++++++++++++++++++++++++++++++++++++-----------
 8 files changed, 2455 insertions(+), 618 deletions(-)

git ls-files --others --exclude-standard
（即上述 4 份 Phase 文档、rust/src/discovery.rs、rust/src/process.rs、rust/src/testdata/README.md、rust/src/testdata/uiautomator2_AndroidManifest.xml）
```


审计结束再次记录：

```text
git status --short --branch
## feature/phase1-environment-discovery
 M docs/RUST_MIGRATION.md
 M rust/src/adb.rs
 M rust/src/apk.rs
 M rust/src/domain.rs
 M rust/src/gui.rs
 M rust/src/ldplayer.rs
 M rust/src/logger.rs
 M rust/src/main.rs
?? docs/PHASE0_SYSTEM_CONTRACT_2026-10-09.md
?? docs/PHASE4_ENGINEERING_NOTES_2026-10-10.md
?? docs/PHASE4_INDEPENDENT_AUDIT_PROMPT_2026-10-10.md
?? docs/PHASE4_INDEPENDENT_AUDIT_REPORT_2026-10-11.md
?? docs/PROJECT_IMPROVEMENT_BACKLOG_2026-10-09.md
?? rust/src/discovery.rs
?? rust/src/process.rs
?? rust/src/testdata/

git diff --stat
 docs/RUST_MIGRATION.md | 10 +
 rust/src/adb.rs        | 205 +++++++--
 rust/src/apk.rs        | 853 +++++++++++++++++++++++++++++++------
 rust/src/domain.rs     | 182 +++++++-
 rust/src/gui.rs        | 686 ++++++++++++++++++++++++++----
 rust/src/ldplayer.rs   | 348 ++++++++++++---
 rust/src/logger.rs     | 129 +++++-
 rust/src/main.rs       | 1110 ++++++++++++++++++++++++++++++++++++++----------
 8 files changed, 2988 insertions(+), 535 deletions(-)

git ls-files --others --exclude-standard
（同初始未跟踪项，另含本报告）
```

执行版本：`rustc 1.98.1`、`cargo 1.98.1`、`Node v24.14.1`、仓库 `.conda` Python `3.12.14`。初始 HEAD/分支与结束时一致；本报告是额外未跟踪审计产物。
没有 reset、clean、checkout 覆盖、tag、push 或发布。没有修改账号/密码/验证码、定位身份或学校服务端状态。审计新增测试直接放在现有 Rust 源文件中；运行产物和便携暂存位于被忽略的 `rust/target/` 或系统临时目录。最终工作区仍包含开始时已有的未提交改动，并新增本报告；完整状态以最终 `git status` 为准。

## 2. 环境与执行边界

- Windows PowerShell；Rust/Cargo、Node.js 和仓库 `.conda\python.exe` 可用。
- `cargo metadata` 与 `zhfd-checkin about` 显示 package version **0.1.0**；about 内嵌 commit 为 HEAD `4cfff0a6abed`。Release 构建来自**脏工作区**，该 commit 字段只代表 HEAD，不代表工作区改动已提交。
- ADB `devices -l` 列出 6 个在线 endpoint。未对这些 endpoint 发 shell/screenshot/app 命令；由于未发现 `ldconsole.exe`，无法证明它们是本项目 LDPlayer 实例，也没有对其执行设备流程。
- `instance list` 在隔离 portable staging 中以 exit 4 返回“LDPlayer ldconsole.exe 未找到”及候选路径；没有运行设备 diagnose/dry-run 的后续 ADB 阶段。
- **没有 live 流程、签到点击、APK 安装、Profile 同步、计划任务创建、GUI 可视化验收、GitHub CI 或 release 发布。**

## 3. 测试矩阵

| 类别 | 命令/操作 | 结果 | 限制/说明 |
|---|---|---|---|
| 格式 | `cargo fmt --manifest-path .\rust\Cargo.toml --all -- --check` | PASS | Windows 本机 |
| 静态分析 | `cargo clippy --manifest-path .\rust\Cargo.toml --all-targets --all-features -- -D warnings` | PASS | warnings-as-errors |
| Rust 单元/反例 | `cargo test --manifest-path .\rust\Cargo.toml --quiet` | PASS：78 个逻辑测试 | `lib` 和 CLI bin 都通过同一 `include!("main.rs")` 测试模块，各报告 78；不是 156 个不同用例；GUI bin 0 个独立测试 |
| Release 构建 | `cargo build --release --manifest-path .\rust\Cargo.toml --bins` | PASS | CLI 与 GUI companion 均构建 |
| Portable | `scripts\portable_smoke.ps1 -ExePath ...\zhfd-checkin.exe -FixturePath ...\ready.png` | PASS | about/profile/config/runtime dirs/vision fixture；不等于干净新机器或真实 GUI 验收 |
| AutoJs6 | `node autojs6/validate_node.js` | PASS | structural smoke |
| Python 编译 | `\.conda\python.exe -m compileall -q scripts` | PASS | 仓库解释器 |
| Python 图像回归 | `\.conda\python.exe scripts/test_image_states.py` | PASS | gray/locating/ready/success 4 个 fixture 均匹配 |
| Release 打包 | `scripts\package_release.ps1 -ReleaseDirectory rust\target\audit-dist-final4` + ZIP 清单断言 | PASS | 仅 README、CLI EXE、GUI EXE；没有 config/logs/captures/reports/账号调试文件；不创建 GitHub Release |
| ADB 盘点 | `adb devices -l` | PASS（只枚举） | 有在线 endpoint，但无 LDPlayer 安装，不对设备操作 |
| Portable 失败级联 | 临时 EXE 目录执行 `instance list`、`run --dry-run` | PASS（预期设备发现失败） | `result.json` schema 2；身份/阶段/恢复字段保留，详见下文 |
| GUI 错配置启动 | 临时目录写入非法 TOML，隐藏启动 GUI companion | **发现 UX 缺陷** | GUI exit 0，只落 `gui-startup-error.log`；没有可见错误提示，详见 finding |
| GitHub 工作流 | 未触发 `ci.yml`/`release.yml` | NOT RUN | 未创建 tag、未 push、未发布；本地构建/打包不代表 GitHub runner 验收 |

## 4. 审计反例、原始发现与修复

### 4.1 APK ZIP 与 Android binary XML

**原始发现（代码 bug，已修复）：**审计基线的 `rust/src/apk.rs` 已有新 ZIP 读取代码和调用/测试，却缺失 `parse_binary_manifest`、`parse_string_pool`、`parse_manifest_start` 与字符串解码器。首次 `cargo clippy/test` 在 `apk.rs:34` 报 `cannot find function parse_binary_manifest in this scope`，测试编译同时报多个解析函数缺失。通过对照 HEAD 仅恢复缺失的 binary XML 函数，保留既有 ZIP/fixture 改动；之后增加以下边界检查：

- 二进制 XML 根 chunk 大小必须与文件长度一致，chunk 长度必须前进且完整落在根 chunk 内；
- string pool header/offset table/string data 边界、attribute 区间、字符串索引和已知属性类型都验证；
- UTF-16 byte length 使用 checked arithmetic 并先切片验证，防止畸形长度触发过量分配；package 不可为空；重复 manifest 节点及重复 package/version 属性拒绝；
- ZIP 中央目录必须完整遍历到 EOCD；重复 `AndroidManifest.xml`、ZIP64、跨磁盘、加密（含 strong-encryption 标志）、本地头与中央目录不一致、CRC/尺寸不一致和 manifest 超长均拒绝；一致的 data descriptor 仍可读取；
- 解压上限 16 MiB，验证 CRC 和实际解压长度。

新增/扩展反例覆盖：真实 UiAutomator2 manifest；stored/deflate ZIP；注释中的伪 EOCD；伪造/截断 EOCD 与中央目录；中央目录 target 后的坏条目；重复 manifest ZIP 条目和重复 XML manifest 节点；local header name/flags/method/CRC/压缩长度/解压长度不一致；payload CRC；ZIP64/跨磁盘/加密/超限；截断 local header；data descriptor；XML chunk/字符串偏移/UTF-16 长度溢出/属性缺失与错误类型。

另有一次测试本身断言写错：payload CRC 的反例应期待 `read_zip_entry` 失败，而不是先 `unwrap()` 再期待 XML parser 失败。该测试断言已改为在 ZIP 解码阶段验证错误，生产 CRC 校验保留。

### 4.2 页面路由、日期语义与点击安全

**原始发现（安全/结果可信度风险，已最小修复）：**原路由逻辑对 hierarchy 全文做 `contains`，孤立的 `点击签到`、`已签到` 或 `签到成功` 足以把未知页面判为 Checkin/Ordinary；首页服务卡也可能出现状态文案。点击门禁虽还有 live/confirm/calibrated/前台/time-window/连续 ready/二次确认，但页面身份不能依赖孤立状态字。修复为 Home/Services 导航标记优先、签到标题和业务状态共同确认 Checkin；登录页优先；首页下滑和服务 tab 后每次都重新验证仍处于已知 Home/Services 路由才可点击导航入口。新增孤立状态词未知页、首页/服务卡状态词和可导航路由反例。

**原始发现（错误成功风险，已修复）：**成功流程先确认绿色视觉和 `签到成功`/`已签到` 文案，但旧 `extract_hierarchy_date` 会从 hierarchy 任意 text/content-desc 抽取唯一日期。临时反例 `<node text="签到成功"/><node text="上次更新 2026-10-11"/>` 期望日期未知，修复前实际为 `Some(2026-10-11)`，断言失败。现只接受带签到成功语义或显式“日期”标签的日期证据，并在 hierarchy 中存在互相冲突日期时 fail closed；无日期/歧义不能置 `success=true`。该反例现作为回归测试通过。

现有与新增策略测试还覆盖 ordinary/field-work/unsupported 区分、非普通模式拒绝 tap、dry-run/live/explicit confirm/calibrated/foreground/time-window/ready frame 门禁、点击 mock 最多一次调用、连续 unknown 计数器在已知帧后归零、日期冲突与格式解析。**没有在真实设备运行上述状态序列或触发 live。**

### 4.3 结果、实例、GUI 与发布

- 隔离 portable 失败运行得到：`schema_version=2`、`run_id=2026-10-11_...`、`instance_index=0`、`instance_name=default`、`serial=127.0.0.1:5555`、`error_stage=device_setup`、`recovery_action` 非空；错误是找不到 LDPlayer。符合“设备初始化前尽可能保留身份”的契约。
- LDPlayer parser 增加畸形 running 值拒绝测试；中文名、坏报告端口/可选尺寸、extra columns 和 reported-port 与 index 推导 serial 分离均有测试。现有有效配置过滤 disabled 实例、TCP serial overflow 及 discovery 重复路径/候选优先级也通过。
- GUI Profile 稳定排序新增 calibrated-first/几何次序/重复调用稳定性测试；日志 evidence 新增显式 `events.jsonl` 导出入口与白名单导出 helper，避免 run 目录优先选中 `result.json` 后无法从 GUI 导出 events 的问题。named export 拒绝路径穿越名称，测试通过。
- Release ZIP 实际清单仅三项：`README.txt`、`zhfd-checkin.exe`、`zhfd-checkin-gui.exe`。未生成 tag/Release。

## 5. Findings（按当前未解决项排序）

### [P2] GUI 配置解析失败没有用户可见提示，进程仍报告成功退出

- 位置：`D:\Documents\Projects\check-in-zhfd\rust\src\bin\zhfd-checkin-gui.rs:3-15`；GUI 加载路径由 `rust/src/main.rs:365-368` 调用 `ConfigStore::load()`。
- 最小复现：隔离目录放入 GUI EXE 与 `config.toml` 内容 `[not valid TOML`，隐藏启动 GUI。
- 实际：GUI 进程 `ExitCode=0`；目录中生成 `gui-startup-error.log`，内容为 TOML parse error；没有观察到 message box、GUI 页面或用户可见错误。由于它是 `windows_subsystem = "windows"`，普通双击时用户可能只看到窗口未出现，难以恢复配置。
- 类型：代码 UX/恢复缺陷，不是配置解析器误接受；未发现凭据泄漏。
- 建议：启动失败时给出原生错误对话框（至少包含配置文件路径和日志位置），或启动一个最小错误窗口/恢复流程；错误路径应采用非零进程码（若平台允许）并加可自动化的启动失败测试。

### [P2 / 发布准备] 工作区/便携包版本仍为 0.1.0，而审计目标标注 v0.2.0 改进

- 位置：`D:\Documents\Projects\check-in-zhfd\rust\Cargo.toml:3`；Release 脚本从 Cargo metadata 读取版本。
- 证据：本地 about 和 package metadata 均输出 `0.1.0`；隔离 ZIP 命名 `zhfd-checkin-windows-x64-v0.1.0-4cfff0a6abed.zip`。
- 类型：版本/发布治理状态，不是运行时 correctness bug。没有擅自 bump 版本、建 tag 或发布；若 `v0.2.0` 是本轮目标版本，需在独立验收通过后同步 Cargo/docs/发布说明并再执行 tag 模拟。

### 未发现的高优先级问题

- 本次静态/单元/portable 证据中，未发现 dry-run 进入签到按钮 tap、外勤/未知模式走普通 live tap、点击后 run-loop 再次 tap、账号/密码/验证码被打包或诊断报告泄露的确定性问题。
- 这**不等同于设备/GUI 的安全验收**：本机没有 LDPlayer，真实 UI 页面切换和 live tap 一次性门禁未在设备上观察；所以不能据此签发“无未授权 tap”的全面保证。

## 6. 未验证项与环境限制

1. 没有 LDPlayer `ldconsole.exe`，无法验证真实注册表多版本选择、`list2` 厂商差异、多个实例顺序/失败级联/设备 Profile/log 目录隔离，以及 ADB offline/unauthorized/connect timeout/screenshot 空内容。
2. ADB 列出 6 个 online endpoint；未对其执行 shell、截图、启动 App 或点击，也没有将其当作 LDPlayer。
3. 未运行真实 hierarchy dump 编码/截断兼容性测试；真实 UiAutomator2 **manifest** fixture 已测，但这不是学校 App hierarchy fixture。
4. GUI 只做了损坏 config 的启动黑盒；未实际渲染小窗口、窄屏、低分辨率、字体、超长路径、100+ 日志、筛选无结果、复制/打开目录等交互。
5. `CREATE_NO_WINDOW`/detached 子进程策略代码集中在 `rust/src/process.rs`，但本次没有用可观察 GUI 设备动态验证闪窗行为。
6. GitHub Actions windows-latest、tag-triggered release、fresh-user profile 均未运行；本地 release package smoke 不是 CI/release workflow 端到端验收。
7. `--all-instances` 的真实顺序/设备失败后继续与 live 多实例流程未执行；只能基于配置过滤/调度代码和单元测试检查。

## 7. 新增测试/代码修改清单

未新增独立 fixture 文件（复用 `rust/src/testdata/uiautomator2_AndroidManifest.xml` 并在测试 helper 中构建畸形 ZIP/XML）。审计改动落在已有工作区改动之上：

- `rust/src/apk.rs`：恢复缺失解析函数；XML/ZIP bounds、全中央目录验证、重复项/加密/尺寸/类型拒绝、UTF-16 checked length 与反例测试。
- `rust/src/main.rs`：路由门禁与未知状态回归；日期证据上下文和冲突日期 fail-closed；对应日期反例测试；移除 dead assignment/Clippy 告警。
- `rust/src/ldplayer.rs`：`list2` running 字段非法值拒绝与边界测试。
- `rust/src/domain.rs`：连续未知帧交错序列测试与 test-only/reserved helper lint 注记。
- `rust/src/discovery.rs`：Windows 大小写路径重复候选测试。
- `rust/src/gui.rs`：Profile 稳定排序测试；事件日志显式导出按钮。
- `rust/src/logger.rs`：受白名单约束的 named artifact 导出 API及事件日志导出测试。
- `docs/PHASE4_INDEPENDENT_AUDIT_REPORT_2026-10-11.md`：本报告。

## 8. 结论

**Phase 4 当前不能判为独立验收通过。** Rust/Node/Python fixture、本地 release 构建和便携包检查通过，且本轮修复了 APK parser/build blocker、路由 fail-closed、成功日期假阳性和事件导出问题；但 GUI 错配置没有可见恢复提示，版本尚未到目标 `0.2.0`，同时真实 LDPlayer、多实例、GUI 视觉和 GitHub workflow 仍未验证。因此结论是：**审计发现已大幅收敛，但保留 P2 findings 和设备/GUI/CI 未验证条件；修复 GUI startup UX、确认版本策略并补完目标环境验证后，才可提交 Phase 4 独立验收。**

本审计没有创建 tag、push 或发布 GitHub Release。

---

## 9. P2 remediation 独立复核（2026-10-11）

> 本节是修复后复核记录；上文 P2 finding 保留为**修复前历史观察**，没有改写原始证据。根据以下独立动态/构建证据，两项 P2 现可关闭。此关闭只针对这两个 finding，不代表整个 Phase 4 通过，也不代表发布。

### P2-1 — GUI 无效配置启动提示：CLOSED

复核文件：`D:\Documents\Projects\check-in-zhfd\rust\src\bin\zhfd-checkin-gui.rs:3-58`。

**非法 TOML 复现**：在 `rust/target/audit-gui-invalid-*` 隔离便携目录中，仅放 GUI EXE 与 `config.toml` 内容 `[not valid TOML`，直接启动 release GUI companion；没有传 CLI 参数，也没有真实设备连接。

- `MainWindowTitle` 检测到真实顶层窗口 `签到助手启动失败`；通过 Windows UI Automation 遍历原生对话框控件读取到“确定”按钮和完整错误正文。
- 对话框正文包含 TOML 解析错误详情、该 staging 目录的完整 `config.toml` 路径、“不会自动覆盖您的配置”及备份/修复/临时改名提示，并给出 `gui-startup-error.log` 完整路径。
- 点击/发送 Enter 关闭“确定”后，进程退出；`System.Diagnostics.Process.ExitCode=1`。
- 日志文件存在，内容含 `GUI 启动失败`、config 绝对路径和 TOML parse error。配置仍为畸形 fixture，没有被程序覆盖。

**日志不能写入反例**：把隔离 staging 中 `gui-startup-error.log` 预先建成同名目录，再以相同非法 TOML 启动。

- 原生窗口仍可见；正文仍含 config 路径和 TOML 详情，并明确提示“启动日志写入失败，请检查程序目录的写入权限”；没有伪称日志文件已保存。
- 关闭对话框后进程仍以 exit 1 退出。该场景确认日志失败不会吞掉可见错误。

**正常路径回归**：全新隔离目录只放 GUI EXE，不放 config；release GUI 正常创建默认 `config.toml` 并显示主窗口 `智汇福大自动签到`。关闭测试窗口后进程 exit 0。此测试没有使用账号或连接设备。

### P2-2 — 版本元数据与便携包版本：CLOSED

复核证据：

- `cargo metadata --manifest-path .\rust\Cargo.toml --no-deps --format-version 1` → `zhfd-checkin@0.2.0`；`rust/Cargo.toml` 与根 package 的 `rust/Cargo.lock` 均为 `0.2.0`。
- 重新执行 `cargo build --release --manifest-path .\rust\Cargo.toml --bins` 后，CLI `--version` 和 `about` 均输出 `zhfd-checkin 0.2.0`。
- GUI About 源码 `rust/src/gui.rs` 读取 `env!("CARGO_PKG_VERSION")`；重建的 `zhfd-checkin-gui.exe` 含编译版本字串 `0.2.0`。GUI About 页面未通过控件导航做视觉截图验收，版本一致性由相同 Cargo 编译环境宏与二进制嵌入字串佐证。
- `scripts/package_release.ps1` 在忽略目录 `rust/target/audit-dist-postp2-*` 执行本地 staging，产生 `zhfd-checkin-windows-x64-v0.2.0-4cfff0a6abed.zip`；README 中 `版本：0.2.0`，与文件名和 Cargo metadata 一致。ZIP 精确 3 项：README、CLI EXE、GUI EXE；没有 config/logs/captures/reports。
- `scripts/portable_smoke.ps1` 使用新 release binaries 通过：`PORTABLE_SMOKE_PASS`。
- `git tag --list v0.2.0` 无输出；没有创建 tag、push 或 GitHub Release。上面 ZIP 是**本地审计用暂存包**，不是发布。

### 修复后回归命令

| 命令 | 修复后结果 |
|---|---|
| `cargo fmt --manifest-path .\rust\Cargo.toml --all -- --check` | PASS |
| `cargo clippy --manifest-path .\rust\Cargo.toml --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --manifest-path .\rust\Cargo.toml --quiet` | PASS：78 个逻辑用例（lib 和 CLI bin 各重复运行同一测试模块） |
| `cargo build --release --manifest-path .\rust\Cargo.toml --bins` | PASS，Cargo 显示 `Compiling zhfd-checkin v0.2.0` |
| `scripts\portable_smoke.ps1 ...` | PASS |
| 本地 `package_release.ps1` + ZIP/README 清单核验 | PASS；不发布 |

### 修复后结论

- 历史 finding `[P2] GUI 配置启动失败没有用户可见提示`：**CLOSED**（原生对话框内容、日志成功/失败、exit 1 和正常启动均有隔离动态证据）。
- 历史 finding `[P2 / 发布准备] 版本元数据仍为 0.1.0`：**CLOSED**（Cargo/CLI/GUI build metadata/便携 ZIP 名称与 README 对齐至 0.2.0）。
- Phase 4 总体验收：**仍未通过/待后续独立验收**。没有 LDPlayer，真实多实例、设备页面状态机、无闪窗动态观察、屏幕尺寸/字体 GUI 验收及 GitHub CI/tag workflow 仍未验证。关闭两个 P2 不消除这些未验证项，也不授权 live 点击或发布。
