# Phase 0：系统契约与基线冻结

> 状态：Phase 0 基线冻结草案完成  
> 冻结日期：2026-10-09（Asia/Shanghai）  
> 代码基线：`v0.1.0` / commit `4cfff0a`  
> 本文是后续 Phase 1–4 的架构约束和验证参照；“目标契约”不代表当前代码已全部实现。

## 1. 文档目的与术语

本阶段只冻结产品行为、领域边界、数据契约和安全不变量，不实施功能变更。

本文使用以下规范性词语：

- **MUST / 必须**：违背即为正确性或安全缺陷；
- **SHOULD / 应当**：默认要求，只有记录理由和风险后才可例外；
- **MAY / 可以**：可选实现，不影响系统契约。

所有契约均标注：

- **现状**：截至基线代码确实存在的行为；
- **目标**：后续实现必须达到的行为；
- **差距**：现状与目标之间尚未完成的工作。

## 2. 产品使命与系统边界

### 2.1 产品使命

本系统是 Windows 上的智汇福大签到辅助工具，通过 ADB 控制 LDPlayer，完成应用启动、页面导航、页面状态观测和（仅在授权后）签到点击。

产品优先级：

1. 不发生未经授权或错误位置的点击；
2. 能解释运行结果和失败原因；
3. 新机器可发现依赖或获得明确配置引导；
4. GUI、CLI、计划任务执行同一业务策略；
5. 运行证据足以支持离线诊断和回归。

“自动运行”不意味着“任何页面都尝试猜测并点击”。未知业务模式必须可识别、可报告、不可静默降级为普通签到。

### 2.2 系统边界

系统负责：

- CLI/GUI 交互；
- 配置读取、校验、保存与恢复；
- LDPlayer/ADB 发现和实例选择；
- App 启动、页面观测、视觉分析；
- dry-run/live 策略执行；
- 结构化日志、结果和诊断报告。

系统不负责：

- 自动获取、保存或绕过学校账号凭证；
- 自动输入密码、验证码或同意协议；
- 修改设备身份、定位数据或 App 服务端状态；
- 推断用户未明确授权的外勤/特殊签到业务；
- 保证 LDPlayer、学校 App、ADB 网络或远端服务本身可用。

## 3. 目标逻辑架构与依赖方向

```text
Presentation
  ├─ GUI
  ├─ CLI
  └─ Windows Task Scheduler adapter
       ↓ 调用同一应用服务
Application Services
  ├─ RunCheckin
  ├─ DiagnoseEnvironment
  ├─ ManageConfiguration
  ├─ InspectProfile
  └─ ExportEvidence
       ↓ 编排领域策略，不包含平台细节
Domain
  ├─ InstanceIdentity
  ├─ PageRoute
  ├─ CheckinMode
  ├─ CheckinState
  ├─ RunResult / RunPolicy
  └─ ClickAuthorization
       ↑ 由适配器提供接口实现
Infrastructure Adapters
  ├─ LDPlayer
  ├─ ADB / Android UI hierarchy
  ├─ Windows process launcher
  ├─ TOML / filesystem
  └─ Screenshot / image classifier
       ↓ 生成可审计证据
Evidence
  ├─ events.jsonl：事件时间线
  ├─ result.json：稳定机器结果契约
  ├─ screenshots/：必要截图证据
  └─ reports/：脱敏诊断报告
```

架构约束：

1. GUI MUST NOT 实现独立的设备发现、点击授权、Profile 判定或视觉状态策略；
2. CLI 与 GUI MUST 调用相同的应用服务；
3. 领域策略 MUST NOT 依赖具体盘符、Windows UI、ADB 命令行或 egui 控件；
4. ADB/LDPlayer/配置文件错误 MUST 转换为稳定错误类型，不以崩溃作为正常恢复路径；
5. 进程启动规则集中在 Windows process adapter，避免各模块自行设置窗口标志。

## 4. 核心领域模型契约

### 4.1 实例身份

每次运行必须尽早确定：

```text
InstanceIdentity {
  index: optional<u32>,
  name: optional<string>,
  adb_serial: optional<string>
}
```

规则：

- `--all-instances` 与单实例 index/serial 互斥；
- 每个实例独立执行、独立结果、独立日志；
- 失败发生在 ADB 连接之前，也应尽可能记录已解析的 index/name/serial；
- `ldplayer_reported_port` 只表示 LDPlayer `list2` 的原始报告列；它与本工具用于连接的 TCP ADB `serial` 分开呈现，不互相推导。

### 4.2 页面路由与业务模式必须正交

页面路线：

```text
Login | Home | Services | Checkin | Unknown
```

签到业务模式：

```text
Ordinary | FieldWork | Unsupported
```

视觉按钮状态：

```text
Gray | Locating | Ready | Success | Unknown
```

三者不能合并成一个枚举或由按钮颜色相互推断。示例：

```text
route=Checkin, mode=FieldWork, state=Ready
```

不等同于：

```text
route=Checkin, mode=Ordinary, state=Ready
```

外勤模式在明确业务规则和授权之前，目标行为是 dry-run 记录并报告，不点击。

### 4.3 状态迁移

普通签到目标迁移：

```text
启动
→ 确认前台 App
→ 路由 Login/Home/Services/Checkin/Unknown
→ 若 Login：停止并提示手动登录
→ 若 Home/Services：有限导航并复核
→ 若 Checkin + Ordinary：检查时间窗并识别状态
→ Gray/Locating：窗口内有限等待
→ Ready：连续帧稳定
→ dry-run: dry_run_ready=true，不点击
→ live: 再次截图与策略授权后单击一次
→ SuccessToday：结束
→ 任何 Unknown/Unsupported/超时：停止并保存证据
```

规则：

- Unknown 不允许转成 Ready；
- 页面路由未确认时，动态彩色候选不得进入可点击分支；
- 导航点击只允许点击可确认的页面入口，坐标 fallback 必须有独立安全论证和回归证据；
- 点击后不得再次 tap；
- 登录页不自动点击“一键登录”。

### 4.4 时间窗口和成功语义

默认窗口：`21:00–23:59（Asia/Shanghai）`。

- Live 点击必须在窗口内；
- dry-run 窗口外不应执行签到按钮点击；
- `success=true` 的语义是“本次确认目标日期的签到成功”，不能仅因绿色按钮而置 true；
- 成功页日期缺失或不等于当天时，目标结果应为 `SuccessStale` 或 `SuccessDateUnknown`，不能报告当天成功；
- 日期比较使用配置时区，而非隐式 Windows 本地时区。

### 4.5 点击授权不变量

Live tap 的必要条件取 AND：

```text
mode == Live
AND explicit_confirmation == true
AND checkin_mode == Ordinary
AND page_route == Checkin
AND profile.calibrated == true
AND foreground_package == configured_package
AND within_window == true
AND consecutive_ready_frames >= required_frames
AND pre_tap_confirmation == Ready
AND this_run_has_not_tapped == true
```

任何条件未知、缺失或不一致均禁止点击。GUI、CLI、计划任务不得绕过此策略。

## 5. 配置契约

### 5.1 生命周期

```text
Locate → Read → Parse → Validate → Normalize → Use
                              ↘ Repair/Backup/Reload
```

目标规则：

- 配置路径来自显式选择、命令行参数或默认 EXE 同目录策略，并必须在 UI 中可见；
- `ConfigInvalid` 是可恢复应用状态，不是 GUI 启动崩溃；
- 重载失败时继续保留上一个有效配置；
- 保存使用临时文件 + 原子替换，关键更改前保留备份；
- GUI 存在后台运行任务时禁止危险配置 reload，或明确让该任务绑定启动时配置快照；
- Windows 路径样例首选 TOML literal string：

```toml
ldconsole_path = 'F:\leidian\LDPlayer14\ldconsole.exe'
adb_path = 'F:\leidian\LDPlayer14\adb.exe'
```

### 5.2 依赖发现优先级

目标优先级：

```text
用户显式配置
> 与已发现 ldconsole 同安装目录/安装树的工具
> 环境变量
> PATH
> 注册表安装位置
> 有限、可解释的常见目录候选
```

发现结果必须带 `source` 和候选诊断。开发机私有盘符不得作为唯一默认路径。多个候选时必须有确定优先级或用户选择，不允许不透明随机选取。

## 6. RunResult 与错误契约

### 6.1 结果分类

目标错误至少分为：

```text
config_invalid
config_unreadable
dependency_not_found
adb_unavailable
instance_not_found
app_start_timeout
login_required
navigation_not_confirmed
unsupported_checkin_mode
unknown_profile
uncalibrated_profile
outside_window
unknown_visual_state
location_timeout
success_stale
success_date_unknown
```

错误码稳定且机器可读；UI 文案可以变化，但不能把错误码当作展示文案。

### 6.2 结果字段

每次运行最终都应输出：

```text
schema_version
run_id
started_at / finished_at
mode
instance_index / instance_name / serial
screen_width / screen_height / dpi / profile_id
page_route / checkin_mode
state_history
clicked / success / dry_run_ready
success_date
error_code / error_stage / recovery_action
artifact_paths
```

不适用字段使用明确 null/unknown，不以空字符串伪装成功。CLI exit code 与错误分类映射由单一函数维护。

## 7. 基线快照（2026-10-09）

### 7.1 Git 基线

```text
Tag: v0.1.0
HEAD: 4cfff0a chore: organize Windows release and CI artifacts
```

工作区基线包含用户提供的未跟踪文档 `docs/RUST_PLAN.md`；Phase 0 新增的契约文档为本阶段产物，均不得被误认为发布源码已经实现目标契约。

### 7.2 当前实际已实现

- CLI/GUI 双 EXE；GUI 使用 Windows GUI subsystem；
- GUI 多数业务调用 CLI 共用核心函数；
- dry-run 默认模式；
- live 有显式确认、标定 Profile、前台包名、窗口、稳定 ready 和点击前复核门禁；
- ADB 启动注册有限重试；
- 支持 9 个 Profile；`portrait_900x1600_d240` 和 `portrait_900x1600_d320` 标记 calibrated；
- 运行日志有 `events.jsonl`、`result.json` 和 screenshots；
- 页面路由有基本 hierarchy 文本判定；
- CI 有 Windows 测试、release build、portable smoke、Python 和 Node 步骤。

### 7.3 已知差距（不可当作已完成）

- config 错误仍可能阻止 GUI 启动，GUI 无 repair state；
- 环境发现未形成统一候选报告，新机器仍可能手动配路径；
- 子进程创建点分散，GUI 子进程闪窗未统一治理；
- 外勤签到无业务模式；`debug/20261008.png` 的蓝色按钮被普通 ready 分类器判断为 ready，但其业务含义不同；
- 成功状态尚未校验成功日期；
- 视觉负样本、route 证据和部分测试覆盖不足；
- GUI Profile 顺序、日志滚动体验仍待改善；
- `RunResult` 不含完整阶段、设备几何和恢复建议；
- `adb_port` 字段语义待修正；
- 干净 Windows 环境与 tag workflow 尚未实际远端验收。

### 7.4 本次本地基线验证

2026-10-09 执行：

| 命令 | 结果 |
|---|---|
| `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | PASS |
| `cargo clippy --manifest-path rust/Cargo.toml --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --manifest-path rust/Cargo.toml` | PASS；lib 39 tests、main 39 tests、GUI bin 0、doc-tests 0（两组 Rust 测试多数为同一 crate 双 target，不记作 78 个不同用例） |
| `scripts/portable_smoke.ps1` | PASS；双 EXE、about/Profile、运行目录和 ready fixture |
| `node autojs6/validate_node.js` | PASS |
| `python -m compileall -q scripts` | PASS |

没有执行真实设备、live、窗口内签到或改系统时间验证。CI 中 Python image regression 要求 `requirements.txt` 依赖；本次基线命令只跑 compileall 和 Node smoke，不声称已运行该图像回归。

## 8. 决策记录

### D-001：GUI 配置错误必须可恢复

决定：无效配置不能导致 GUI 无提示退出。  
理由：配置是用户可编辑输入，不应把输入错误升级为程序不可用。  
影响：GUI 启动入口必须允许无效配置进入 Repair UI。

### D-002：外勤签到与普通签到分开建模

决定：外勤页面不复用普通 `Ready` 的 live 授权。  
理由：业务文案、地理语义和状态流程不同，颜色一致不证明动作等价。  
影响：先 dry-run 识别和记录，业务确认前 live 禁止。

### D-003：成功必须对应当天

决定：视觉绿色仅是视觉状态，不足以证明本日任务成功。  
理由：新平台日志显示旧日期成功页面可被后续运行观察到。  
影响：结果必须附成功日期证据；日期缺失不得断言 SuccessToday。

### D-004：未知状态不得隐式升级

决定：未知 route/mode/state 只能停止、重试或请求用户，不可转成普通 ready。  
理由：保持 fail-closed，同时通过可恢复提示提高可用性。

### D-005：实例间隔离是强不变量

决定：一个实例的 ADB、Profile、页面、点击状态、日志和结果不能用于另一个实例。  
影响：任何 all-instances 服务都必须是逐实例独立运行与结果聚合。

## 9. Phase 0 退出条件

- [x] 写明系统使命和外部边界；
- [x] 明确 GUI/CLI/Application/Domain/Infrastructure/Evidence 依赖方向；
- [x] 冻结实例、路由、业务模式、视觉状态、点击、时间和 success 契约；
- [x] 冻结配置生命周期、错误恢复和路径规范；
- [x] 冻结结果字段、错误码方向和证据模型；
- [x] 记录当前基线实现与目标的差异；
- [x] 本机不触设备的格式、lint、测试、portable、Node/Python compile 基线通过；
- [x] 明确禁止未经用户授权的 live、tag 和推送。

Phase 0 完成仅代表“设计输入和行为基线已冻结”，不代表上述目标已实现，也不代表新机器问题或外勤签到已经解决。

## 10. Phase 1 输入

Phase 1 开始前应先逐项把本契约映射到实现设计：

1. EnvironmentDiscovery API 和候选优先级；
2. ConfigService 生命周期、GUI Repair 状态和 reload 事务；
3. Windows ProcessLauncher 能力及 stdout/stderr 传递；
4. `RunEvidence` schema_version 与向后兼容；
5. Phase 1 测试矩阵和清洁环境模拟方式。

未完成这些设计映射前，不应把所有 P0 需求并行拆成彼此独立的热修复。