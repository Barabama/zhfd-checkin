# 项目系统性改进路线图

> 更新时间：2026-10-11（Asia/Shanghai）  
> 项目：`check-in-zhfd`  
> 文档性质：架构级改进规划、质量治理和阶段交付清单。  
> 当前约束：Phase 1–3 实施在 `feature/phase1-environment-discovery`；所有测试、验收和最终验证交由独立测试端 agent；实现阶段不执行验证工作，不创建 tag、不推送、不发布。

## 0. 为什么重新规划

当前反馈表面上是 8 个 bug：路径找不到、GUI 配置不好用、路径转义、Profile 排序、日志滚动、外勤页面、终端闪现、日志诊断困难。

但这些问题并不是 8 个互不相关的小问题，而是同一个系统性缺口的不同表现：

```text
部署环境不确定
    ↓
依赖发现和配置生命周期不可靠
    ↓
GUI 无法恢复、用户只能手工编辑
    ↓
设备/页面状态缺少领域模型
    ↓
错误只能落成一个字符串
    ↓
日志可看但不可诊断，发布只能依赖人工经验
```

因此后续不采用“发现一个问题就改一行”的方式，而采用：

```text
产品目标
→ 领域边界
→ 目标架构
→ 质量属性
→ 风险优先级
→ 阶段门
→ 实现任务
→ 验收证据
```

## 1. 软件工程设计原则

### 1.1 先定义系统承诺，再定义功能

当前系统对用户的核心承诺应明确为：

1. 在 Windows 新机器上能够被引导配置，而不是要求用户理解 Rust/TOML/ADB；
2. 设备不可用、配置错误、未登录、业务页面不支持时，系统必须可解释、可恢复、fail-closed；
3. dry-run 是默认安全运行模式；
4. GUI 和 CLI 使用同一业务核心，但交互层不能复制设备和视觉判断；
5. 每次运行都能回答：运行了哪个实例、连接了哪个设备、处于哪个页面、识别到什么状态、为什么结束；
6. 发布包不携带账号、配置、日志、截图和设备身份数据。

### 1.2 关注点分离

系统应逐步分成以下层次：

```text
Presentation Layer
  GUI / CLI / scheduled task

Application Layer
  diagnose / run / config reload / report / package operations

Domain Layer
  instance target / page route / check-in mode / button state / run policy

Infrastructure Layer
  ADB / LDPlayer / Windows process / filesystem / TOML / screenshots

Evidence Layer
  events.jsonl / result.json / captures / reports / diagnostics
```

GUI 不应直接知道 ADB 搜索细节；视觉分类器不应决定是否允许 live；计划任务不应拥有另一套运行逻辑；配置错误不应通过进程崩溃表达。

### 1.3 失败不是异常分支，而是产品状态

以下状态必须被建模，而不是只拼接错误字符串：

```text
ConfigMissing
ConfigInvalid
DependencyNotFound
DeviceOffline
DeviceStarting
AppNotForeground
LoginRequired
HomePage
ServicePage
CheckinPage
FieldWorkCheckinPage
UnsupportedCheckinMode
Locating
Ready
SuccessToday
SuccessStale
OutsideWindow
UnknownPage
UnknownVisualState
```

每个状态应有：

- 稳定 machine code；
- 用户可读说明；
- 是否可自动重试；
- 是否需要用户操作；
- 是否允许继续到下一阶段；
- 是否允许 live 点击。

### 1.4 安全性与可用性同时设计

安全策略不是简单地“遇到不认识就退出”。正确目标是：

```text
不误点击
+ 明确告诉用户为什么停
+ 给出可执行的恢复动作
+ 保留足够证据供诊断
```

例如外勤页面当前不能直接当作普通 ready，但也不应只显示 `navigation_not_confirmed`；应明确显示“检测到外勤签到模式，当前策略不支持自动点击”。

### 1.5 可测试性优先于本机便利

不得把开发机路径、真实设备、当前 Windows 注册表或当前时间作为单元测试前提。设备相关逻辑应通过接口和 fixture 测试，真实窗口只承担集成验证，不承担所有回归责任。

## 2. 当前系统基线

- 当前 tag：`v0.1.0`，对应提交 `4cfff0a`；
- 当前 GUI/CLI 双 EXE 架构已建立；
- 当前三个 LDPlayer 实例均曾使用 `900x1600@320`，兼容 Profile 为 `portrait_900x1600_d320`；
- 默认签到窗口为 `21:00–23:59（Asia/Shanghai）`；
- dry-run 计划任务保留；不创建 live 自动任务；
- `v0.1.0` 之后发现的新机器可用性问题尚未进入发布修复；
- 用户提供的 [RUST_PLAN.md](./RUST_PLAN.md) 保持未跟踪，不自动纳入功能提交；
- 新平台证据位于 `debug/`，不应直接进入发布包。

当前核心实现边界：

- 配置加载：`rust/src/config.rs`；
- ADB 发现和执行：`rust/src/adb.rs`；
- LDPlayer 发现和实例生命周期：`rust/src/ldplayer.rs`；
- CLI 应用编排、导航和状态机：`rust/src/main.rs`；
- GUI 展示与后台任务：`rust/src/gui.rs`；
- Profile 常量：`rust/src/profile.rs`；
- 视觉分类：`rust/src/vision.rs`；
- 日志/报告：`rust/src/logger.rs`；
- GUI 入口：`rust/src/bin/zhfd-checkin-gui.rs`。

## 3. 反馈证据与系统性归因

> 本节描述的是 `v0.1.0`（commit `4cfff0a`）基线时的代码路径和问题归因；后续实现状态见 Phase 1 进度记录。

### 3.1 新机器找不到 LDPlayer/ADB

现象：安装雷电模拟器后必须手动填写 `ldconsole_path` 和 `adb_path`。

代码路径：

- `LdPlayer::discover`：候选路径、PATH、注册表和配置读取；
- `find_adb`：ADB 配置、环境变量、开发机硬编码路径和 PATH 查找；
- `runtime_with_target`：把依赖发现、ADB 连接、设备信息读取直接串在运行流程中。

系统性归因：当前没有独立的“依赖发现领域”和“发现结果模型”，发现逻辑以过程函数存在，GUI 无法展示候选、来源和失败阶段。

### 3.2 GUI 没有配置浏览、重载和错误恢复

现象：GUI 只能打开当前 config，不能选择其他配置；错误 TOML 可能导致 GUI 启动失败，用户只看到退出。

代码路径：

- `ConfigStore::load` 在 GUI 启动前直接返回错误；
- GUI `config_page` 只展示文本和打开编辑器；
- GUI 独立入口失败时只写 `gui-startup-error.log`。

系统性归因：配置被当作“启动前静态文件”，没有被建模为用户可管理的运行时资源；GUI 没有 Recovery/Repair 状态。

### 3.3 Windows 路径转义

现象：用户用 TOML 基础字符串填写 Windows 路径时容易写错反斜杠。

系统性归因：配置格式的机器语法暴露给了普通用户；产品没有通过模板、路径选择器、规范化写回和错误提示降低认知负担。

### 3.4 Profile 排列与日志界面可用性

现象：Profile 顺序不稳定/不符合用户优先级；日志表格没有双向滚动。

系统性归因：GUI 仍是“把内部数据结构直接画出来”，没有独立的 View Model、排序策略、表格交互和信息密度设计。

### 3.5 外勤签到页面

证据：[debug/20261008.png](../debug/20261008.png) 显示：

- 页面标题为“晚点名签到”；
- 日期为 `2026-10-08`；
- 状态为“未签到”；
- 按钮文案为“外勤签到”；
- 页面包含时间和当前位置；
- 页面不是普通“点击签到/定位中...”视觉模板。

代码路径：

- `detect_page_route`：主要根据 hierarchy 文本判断页面；
- `locate_or_navigate`：路由未确认时返回 `navigation_not_confirmed`；
- `find_colored_button/classify_button`：只建模 gray/locating/ready/success；
- 主运行循环：只接受既有 ButtonState，不理解业务按钮文案。

系统性归因：当前把“页面导航”“业务模式”“按钮视觉状态”混在一起。外勤签到不是单纯的视觉阈值问题，而是业务模式扩展问题。

### 3.6 ADB 终端闪现

系统性归因：Windows 子进程启动策略没有抽象成平台服务；GUI、CLI、诊断、ADB、LDPlayer 和计划任务各自直接创建进程。

### 3.7 新平台日志

`debug/logs/` 反映出多类失败：

- `unknown_profile`；
- `navigation_not_confirmed`；
- `等待 App 前台超时`；
- `outside_window`；
- `location_timeout`。

系统性归因：运行结果字段偏少，错误阶段、设备几何、页面路线和恢复建议没有形成统一诊断模型；日志虽有事件，但还不是完整的运行证据链。

## 4. 目标架构

### 4.1 环境适配层：Environment Adapter

新增概念层，不要求立即新增文件名，但架构上必须独立：

```text
EnvironmentDiscovery
  ├─ discover_ldplayer() -> ToolCandidate
  ├─ discover_adb(ldplayer_candidate) -> ToolCandidate
  ├─ discover_instances() -> InstanceInventory
  └─ diagnostics() -> DiscoveryReport
```

`ToolCandidate` 至少包含：

```text
path
source: config | env | path | registry | sibling | conventional
exists
version/identity if available
warnings
```

规则：

- 用户显式配置优先；
- 自动发现必须记录来源；
- ADB 与 LDPlayer 必须进行一致性检查；
- 不再把开发机盘符当作默认策略；
- 失败时返回候选路径和下一步操作。

### 4.2 配置生命周期：Configuration Service

配置不再只是 `ConfigStore::load()`，而是具有生命周期：

```text
Locate
→ Parse
→ Validate
→ Normalize
→ Present
→ Edit/Select
→ Reload
→ Persist/Backup
```

配置服务应提供：

- `load(path)`；
- `validate(config)`；
- `format_error(error)`；
- `save_atomic(path)`；
- `backup(path)`；
- `reload()`；
- `reset_default()`；
- `path_for_display()`。

GUI 启动必须支持三种状态：

```text
ConfigReady
ConfigMissing
ConfigInvalid
```

`ConfigInvalid` 不是进程终止，而是进入修复页。

### 4.3 设备控制层：Device Gateway

ADB、LDPlayer 和 Windows 进程需要通过统一 gateway 暴露：

```text
DeviceGateway
  ├─ ensure_tooling()
  ├─ ensure_instance_running()
  ├─ connect(serial)
  ├─ current_device_info()
  ├─ launch_app()
  ├─ screenshot()
  ├─ hierarchy()
  ├─ tap()/swipe()
  └─ process_diagnostics()
```

GUI、CLI、计划任务只能调用 gateway/application service，不直接拼接 `Command::new`。

### 4.4 业务页面层：Check-in Session

签到流程应分成三个独立模型：

```text
PageRoute
  home / services / checkin / login / unknown

CheckinMode
  ordinary / field_work / unsupported

CheckinState
  gray / locating / ready / success_today / success_stale / unknown
```

只有以下组合允许进入 live policy：

```text
route=checkin
mode=ordinary
state=ready
profile=calibrated
window=true
foreground=true
confirmation=true
```

外勤页面默认：

```text
mode=field_work
live_allowed=false
result=unsupported_checkin_mode 或 field_work_ready（仅 dry-run）
```

### 4.5 Evidence Model

统一结果结构建议扩展为：

```text
RunEvidence
  run_id
  started_at / finished_at
  mode
  instance identity
  device geometry
  tool paths and versions
  page_route
  checkin_mode
  visual states
  current_date / detected_success_date
  error_stage
  recovery_action
  clicked / success / dry_run_ready
  artifact paths
```

`result.json` 是机器接口；`events.jsonl` 是时间线；截图/hierarchy 是证据附件；GUI 是它们的视图，不应自行推导另一套状态。

## 5. 质量属性目标

### 可部署性

- 新机器不依赖开发机盘符；
- 第一次运行有诊断和引导；
- 配置错误可恢复；
- 发布包和运行时数据分离。

### 安全性

- 默认 dry-run；
- 外勤模式默认不 live；
- 首页/登录页/未知页不可进入点击状态机；
- success 必须校验当天日期；
- 所有设备控制失败 fail-closed。

### 可维护性

- 设备发现、配置、页面路由、业务模式和视觉分类可独立测试；
- GUI 不复制 CLI 业务判断；
- 错误码稳定，不依赖中文字符串匹配。

### 可观测性

- 每次运行可以还原完整状态时间线；
- 失败包含阶段、设备、页面、恢复建议；
- GUI 可以打开和复制证据；
- 日志归档可脱敏。

### 可测试性

- 单元测试不依赖本机路径和真实设备；
- fixture 覆盖普通页、登录页、外勤页、灰/蓝/绿状态和错误配置；
- 集成测试覆盖新机器路径发现、配置修复、隐藏子进程和便携包。

## 6. 分阶段实施路线

### Phase 0：基线冻结与契约定义 — COMPLETE（2026-10-09）

Phase 0 的目标是先冻结“当前实际行为”和“后续必须满足的目标契约”，不是把规划误写成已完成实现。

冻结文档：

- [PHASE0_SYSTEM_CONTRACT_2026-10-09.md](./PHASE0_SYSTEM_CONTRACT_2026-10-09.md)

已完成的设计产物：

- 系统使命、范围、非目标；
- GUI/CLI、Application、Domain、Infrastructure、Evidence 的目标依赖方向；
- 实例身份、页面路线、签到模式、视觉状态、成功日期和点击授权契约；
- 配置生命周期与错误恢复契约；
- RunResult/Evidence 字段与错误码方向；
- 当前 `v0.1.0` 行为基线、已知差距和决策记录；
- Phase 1 设计输入和禁止越过的安全边界。

基线验证（2026-10-09，本地、非设备集成）：

```text
cargo fmt --all -- --check                         PASS
cargo clippy --all-targets --all-features -D warnings PASS
cargo test                                          PASS（lib 39、main 39）
portable_smoke                                      PASS
node autojs6/validate_node.js                      PASS
python -m compileall -q scripts                    PASS
```

未执行真实设备、live、窗口内签到或系统时间修改。上述测试通过只证明当前基线具备这些离线质量证据，不代表新机器问题、外勤签到或 GUI 恢复功能已完成。

Phase 0 退出门：已满足“契约、基线、差异和验证证据可供后续阶段共同引用”。此后如果修改点击授权、业务模式、成功语义或结果 schema，必须先更新契约和兼容策略，再实现。

禁止事项仍有效：

- 不直接增加未经验证的 live 点击路径；
- 不在没有负样本和 fixture 的情况下放宽页面路由门禁；
- 不把外勤按钮改名成普通 ready；
- 不把 dry-run 的旧日期 success 当作当天成功。

### Phase 1：新机器首次运行可恢复

范围：P0。

1. 统一 LDPlayer/ADB discovery；
2. 配置路径选择、重载和 Repair 页面；
3. TOML 路径帮助与规范化写回；
4. 所有 GUI 失败改为可见状态；
5. Windows 子进程隐藏窗口封装。

阶段门：

- 新机器无手填路径可完成诊断；
- 错误 TOML 不退出 GUI；
- GUI 诊断无黑色闪窗；
- CLI 仍保留可诊断 stderr。

### Phase 2：业务页面兼容性

范围：P1。

1. 普通签到、外勤签到、登录页、未知页建模；
2. hierarchy + 导航动作 + 视觉证据的联合路由；
3. 当天 success 校验；
4. 登录/前台超时给出用户动作；
5. 外勤模式默认 dry-run 报告。

阶段门：

- 外勤页面有明确结果码；
- 普通页面状态机回归不退化；
- 不新增未经验证的 live 点击路径；
- 新平台日志可以定位到具体阶段。

#### Phase 2 实施进度（2026-10-10）

状态：**业务模型与运行流程实现完成；验证和验收未执行，等待最终独立测试端 agent。**

本阶段实现范围：

- `PageRoute`、`CheckinMode`、`CheckinState` 成为独立领域枚举，不再用按钮颜色推导业务模式；
- hierarchy 路由增加外勤页面的显式证据要求；只在已知首页/服务页内进行有限滑动和可访问性标签导航，移除无页面证据时的服务图标坐标 fallback；每次导航动作后重新读取层级；
- 普通签到只有在页面、业务模式及视觉状态均可解释时进入状态机；运行中和 live 点击前复核页面/模式，Live 授权只接受 `Checkin + Ordinary`；
- FieldWork 仅做单次 dry-run 观察：层级同时确认签到页标题、未签到状态和外勤按钮时输出 `field_work_ready`，不点击；Live 明确拒绝；证据不足时 fail-closed；
- success 需要成功文案、绿色视觉和 hierarchy 日期三方证据；日期按配置时区比较，stale/date-unknown 不会置 `success=true`；
- App 前台等待、登录要求、导航未确认、页面上下文变化、未知模式和成功日期失败写入机器结果与恢复建议；GUI 展示页面、模式、语义状态、日期、失败阶段和建议操作；
- `RunResult` 增加带 serde 默认值的结构化字段，旧日志可继续读取；日志不保存原始 hierarchy，避免把姓名/定位等页面内容作为诊断副产物扩散。

实现阶段遵循委托边界：**没有运行 cargo test、设备 dry-run/live、GUI 验收或发布 smoke；不把 Phase 2 标记为已测试/已验收。** 最终独立测试端需要单独覆盖普通签到、外勤签到、登录/首页/未知页、成功日期、页面切换和点击授权拒绝路径。

### Phase 3：GUI 信息架构与诊断体验

范围：P2。

1. Profile 稳定排序和 calibrated 置顶；
2. 日志双向滚动、固定操作区、筛选、复制、打开目录；
3. 配置/诊断/运行/证据统一导航；
4. 结果 JSON 和报告导出；
5. 脱敏截图和日志策略。

阶段门：

- 低分辨率窗口可用；
- 长路径/大量日志可用；
- 用户不需要 PowerShell 才能完成常见恢复动作。

#### Phase 3 实施进度（2026-10-10）

状态：**GUI 信息架构与证据浏览功能已实施；按职责约定未执行测试/验收，待最终独立测试端 agent。**

本阶段实现范围：

- 将顶部横向页签改为左侧分组导航：工作台、业务配置、任务与证据、帮助；主内容区不改变 CLI/Application 业务调用路径；
- Profile 按已标定优先、方向、尺寸、DPI 和 ID 稳定排序，未标定 Profile 在 UI 明示为“仅 dry-run”；About 共用相同排序；
- 运行日志页扩展到最多 100 条，可按类型/状态/摘要/路径筛选；结果表格提供水平和垂直滚动，长摘要和路径截断显示；
- 日志操作栏不随表格滚动：刷新、打开 logs 目录、导出/打开诊断报告、筛选、打开选中目录、复制路径/证据、导出结果或同步 JSON；选中证据提供可滚动预览；
- `logger` 增加选择、读取和导出运行/同步/事件证据的共用文件服务；系统资源管理器启动走统一 Windows process adapter；
- 首页和诊断视图展示结构化页面、签到模式、状态、日期、错误阶段和恢复建议；诊断导出的 result.json 与报告均可在 GUI 中查看/复制/导出。

**未执行 GUI 手动验收、窗口缩放检查、cargo test/clippy/build、portable smoke 或设备操作。** 需要最终独立测试端验证低分辨率布局、滚动/筛选/选择联动、长路径、中文复制、结果/报告导出，以及普通/外勤结果的展示没有混淆。

### Phase 4：工程补齐与发布治理

范围：P3。

#### Phase 4 实施进度（2026-10-11）

状态：**Phase 4 工程代码已落地；独立审计后发现的两个 P2 已完成实现侧修正，但尚待独立审计 Agent 复核，Phase 4 仍未验收。当前 Cargo package 版本为 0.2.0，尚无 v0.2.0 tag/release。**

已落地：

- 真实 UiAutomator2 binary manifest fixture 及 APK ZIP/XML 解析边界强化；
- 暂不引入 UiAutomator crate，继续以 `adb shell uiautomator dump` 作为基础设施适配边界，避免在未形成兼容承诺前扩大依赖；
- `ldplayer_reported_port` 替代语义不明确的 `adb_port`，实际设备连接仍以 `serial` 为准；
- RunResult schema v2 方向的运行 ID、时间、设备几何、失败阶段、恢复建议和证据路径；ADB/LDPlayer 初始化失败时尽可能保留实例身份；
- unknown 视觉状态改为连续帧计数器；
- 添加 [Phase 4 工程记录](./PHASE4_ENGINEERING_NOTES_2026-10-10.md) 和 [真实 manifest fixture 说明](../rust/src/testdata/README.md)。
- 针对独立审计 P2 增加 GUI 启动失败可见恢复对话框及非零 exit code，并将 package 元数据更新为 0.2.0；复核细节见 [P2 remediation 记录](./PHASE4_P2_REMEDIATION_2026-10-11.md)。该修正尚未被独立审计 Agent 复核。

本阶段未执行测试、动态验证、真实设备操作、干净 Windows 验收或 release tag 流程；后续由独立审计 agent 根据仓库状态执行完整测试和反例审计。

1. UiAutomator crate 评估；
2. APK 真实 manifest fixture；
3. unknown 连续帧语义；
4. 修正 `ldplayer_reported_port` 语义；
5. 失败结果身份字段完整；
6. 干净 Windows 验收；
7. CI tag/release 远端实测；
8. 文档、发布包和版本契约同步。

## 7. 任务分解与优先级

### P0-1：依赖发现服务

输入：现有 config、注册表、PATH、LDPlayer 目录。  
输出：`DiscoveryReport`、候选来源、可操作错误。  
测试：纯路径 fixture、注册表文本 fixture、空环境 fixture。  
风险：误找到多个版本；必须提供选择和来源。

#### 实施进度（2026-10-10）

状态：**核心实现及离线验证完成；新机器真实安装环境验收仍未完成，因此 Phase 1 阶段门保持开启。**

已完成：

- 新增 `rust/src/discovery.rs` 的候选/报告模型：存在性、来源、确定优先级、去重和可解释摘要；
- LDPlayer 发现覆盖 config（相对路径按程序根目录解析）、程序目录、环境变量、PATH、匹配到 LDPlayer 的卸载注册表项、厂商注册表项和有限常见目录；
- ADB 发现覆盖 config、已发现 LDPlayer 安装树、Android 环境变量和 PATH；移除开发机私有盘符候选；
- CLI `diagnose` 和 GUI 配置页复用同一依赖诊断服务；GUI 可异步扫描、查看来源并在多个已存在候选中显式选择后保存；
- 增加 Windows 子进程启动封装 `rust/src/process.rs`，ADB/LDPlayer/注册表查询使用隐藏窗口启动；此项是 P0-3 的实现切片，不代表闪窗验收已完成；
- 增加路径优先级/去重/空报告/注册表文本/卸载项筛选/相对路径/ADB sibling fixture 测试。

2026-10-10 离线验证：

```text
cargo fmt --all -- --check                                      PASS
cargo clippy --all-targets --all-features -- -D warnings         PASS
cargo test                                                       PASS（lib 50、main 50；同一 crate 双 target，非 100 个独立用例）
cargo build --release --bins                                     PASS
scripts/portable_smoke.ps1                                       PASS
node autojs6/validate_node.js                                    PASS
python -m compileall -q scripts                                  PASS
```

尚未完成：

- 在未配置路径的新 Windows 用户环境确认真实雷电安装布局能被注册表/常见目录找到；
- 实测多个 LDPlayer 安装版本的候选选择和最终运行可用性；
- 在真实 GUI 中确认子进程没有前台终端闪现。

以上未执行 ADB 设备诊断、dry-run、live 或签到点击。代码构建和 portable smoke 只验证便携 CLI/GUI 包装契约，不等同于新机器依赖发现验收。

### P0-2：配置恢复工作流

输入：缺失/损坏/用户选择的 TOML。  
输出：GUI Repair 页面、备份、原子写回、重载。  
测试：语法错误、路径转义、权限错误、旧配置保留。  
风险：重载破坏后台任务；重载时必须禁止并发运行。

### P0-3：Windows 子进程策略

输入：ADB、LDPlayer、where、reg、schtasks、cmd 启动点。  
输出：统一进程启动器和 GUI 隐藏策略。  
测试：GUI 诊断/检测/运行无闪窗；CLI 输出不回归。  
风险：隐藏 stderr 导致诊断变差；必须将输出收集进日志/错误对象。

### P1-1：外勤模式模型

输入：外勤截图、hierarchy、当前定位和签到时间文本。  
输出：`field_work` 模式、dry-run 结果码、GUI 提示。  
测试：外勤 fixture、普通签到 fixture、登录页 fixture。  
风险：业务语义未确认；默认只能报告不能点击。

### P1-2：成功日期与结果可信度

输入：success 页面日期/时间、Asia/Shanghai 当前日期。  
输出：`success_today`、`success_stale`、日期证据。  
测试：当天、前一天、日期缺失、跨午夜。  
风险：OCR/hierarchy 文本缺失；缺失时不能宣称当天成功。

### P2-1：GUI 信息架构

输入：配置、Profile、运行日志、诊断报告。  
输出：侧边栏、双向滚动、固定操作区、统一排序。  
测试：窗口缩放、长路径、长目录、多日志、中文字体。  
风险：UI 重构影响现有操作；先保留旧入口并做导航迁移。

实现状态：已完成 Phase 3 代码落地；独立测试端尚未执行最终验证和验收。实现范围及后续验收委托见“Phase 3 实施进度”。

## 8. 发布与变更控制

当前阶段不做热补丁，不创建 tag，不推送。后续每个阶段必须满足：

1. 设计契约先更新；
2. 代码修改有单元/fixture 测试；
3. 新平台离线日志可复现；
4. 不降低 fail-closed 门禁；
5. 重新构建 CLI/GUI；
6. portable smoke 和 CI 通过；
7. 文档与版本状态同步；
8. 由用户决定是否进入下一阶段或发布。

## 9. 当前明确不做

- 不自动输入账号、密码、验证码；
- 不自动点击“一键登录”；
- 不创建 live 自动计划任务；
- 不绕过 calibrated、时间窗口、前台包名和 ready 门禁；
- 不因为外勤页面出现蓝色圆形就直接开放 live；
- 不把单次真实窗口成功当作所有 Profile/所有业务模式已验证。

## 10. 最终成功标准

项目达到下一版本发布候选的条件不是“8 个反馈都各自修一遍”，而是：

```text
新机器可发现或可引导配置
+ 配置错误可恢复
+ GUI/CLI 共享同一应用服务
+ 页面/业务模式显式建模
+ 外勤与普通签到不混淆
+ 运行结果可解释、可复现、可脱敏
+ 子进程行为符合 Windows GUI 体验
+ dry-run 计划任务可持续回归
+ CI/便携包/文档版本一致
```

只有满足以上系统级条件，才考虑下一次正式发布；在此之前，所有 live 自动任务保持关闭。