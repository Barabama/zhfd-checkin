# CLI 命令面审计（2026-10-21）

- 审计对象：`v0.3.3` tag `de2cc8d`；审计时 `master` HEAD `27f5649`（含 PII remediation 的未提交工作树另见说明）
- 性质：**AI 辅助设计审计，不是独立验收**；只读，不修改命令行为
- 目标：判断 CLI 是否“扁平、低耦合、适合新人快速上手”，并给出目标命令面与迁移路径
- 证据：`cargo run -- --help` / `run --help`、`rust/src/main.rs`、`rust/src/gui.rs`、`docs/QUICK_START.md`、`docs/OPERATIONS.md`

---

## 1. 结论

| 问题 | 结论 |
|---|---|
| 是否扁平？ | **结构上已经扁平**：最大嵌套 2 层，12 个一级命令；不是深层命令树问题 |
| 是否低耦合？ | **CLI/GUI/状态机边界耦合偏高**：clap 参数结构就是 GUI 的内部 API，`RunArgs.confirm` 直接进入点击授权路径 |
| 是否适合新人？ | **目前不够**：`--help` 一级命令描述全空、安全开关无说明、没有 CLI 首次使用路径；新人只能读文档或源码 |
| 是否有 P0？ | 无；安全门禁仍 fail-closed |
| 最高级别问题 | **P1**：`task create --all-instances --serial X` 会注册一个每天必然失败的任务；`--help` 发现性缺失 |

一句话：**扁平度不是主要问题，命令面的一致性和可发现性才是；耦合主要发生在 clap 类型、GUI 和 `RunArgs` 进入状态机这三处。**

---

## 2. 当前命令面

```text
zhfd-checkin [--config PATH] <COMMAND>

diagnose [--instance-index|--serial]
report
profile  list | detect [target] | sync --id ID --confirm [target]
run      [--dry-run|--live] [--confirm] [--all-instances] [target]
app      install --apk PATH [target]
config   show | open
about
gui
instance list
task     create [--name] [--live --confirm] [--all-instances] [target]
         | delete [--name] | query [--name]
ui       dump [target]
vision   --image PATH
```

目标 `target` = `--instance-index N` 和/或 `--serial S`（不是 clap 互斥组）。

---

## 3. 发现

### P1-1 `task create --all-instances` 与 target 组合会静默生成坏任务

证据：

- [main.rs:1104-1131](../rust/src/main.rs#L1104-L1131) `build_task_command_line`：
  - `--all-instances` 时 `--instance-index` 被 `else if` 静默丢弃；
  - `--serial` 却无条件追加。
- [main.rs:1431-1433](../rust/src/main.rs#L1431-L1433) `run_all_instances` 在 `instance_index` 或 `serial` 存在时直接 `bail`。
- [main.rs:167-189](../rust/src/main.rs#L167-L189) `TaskCommand::Create` 没有 clap 冲突约束。

后果：

- `task create --all-instances --serial X` 打印成功并注册 DAILY 任务，但每次触发都在设备操作前 exit 4；
- `task create --all-instances --instance-index 0` 静默丢弃 index，扩大为全部 enabled 实例；
- 与 `run` 的同类组合在运行期才报错，且被顶层映射成 exit 4（见 P2-5）。

修复：在 `RunArgs` 和 `TaskCommand::Create` 上把 `--all-instances` 设为 `conflicts_with_all = ["instance_index", "serial"]`，并把 target 做成 clap ArgGroup；加解析测试。

### P1-2 `--help` 几乎不可用于发现

证据：

- [main.rs:86-123](../rust/src/main.rs#L86-L123) `CommandKind` 没有 about/长描述；`ProfileCommand`/`AppCommand`/`ConfigCommand`/`InstanceCommand`/`TaskCommand`/`UiCommand` 同样。
- [main.rs:198-209](../rust/src/main.rs#L198-L209) `--dry-run`、`--live`、`--confirm` 没有 help。
- 实测 `zhfd-checkin.exe --help`：

```text
Commands:
  diagnose
  report
  profile
  run
  app
  config
  about
  gui
  instance
  task
  ui
  vision
```

后果：新人无法从第一发现入口区分 `diagnose` / `report` / `vision` / `ui dump`，也看不出 `--live --confirm` 是真实点击授权。

修复：每个 command/subcommand 加一句话 about；每个 flag 加 help；顶层 `after_help` 给出三条首次使用命令和安全契约；用 `help_heading` 分组“只读 / 安全演练 / 危险”。

### P2-1 非 GUI 命令统一预加载配置，纯本地命令被损坏配置阻断

证据：[main.rs:239-240](../rust/src/main.rs#L239-L240) 在分发前对所有非 about/gui 命令调用 `load_store_for_cli`；但 `vision --image`、`profile list`、`task query/delete` 不使用 store。

后果：配置损坏时，离线图片分析、profile 列表、计划任务查询/删除都被 exit 4 阻断；运维清理任务反而被配置问题挡住。

修复：按命令声明 `ConfigRequirement { None, Optional, Required }`，把加载下沉到 handler；`task create` 目前还会加载两次（`run_cli` 与 `task_command`），应一并去掉。

### P2-2 `run` 与 `task create` 的 mode 默认不一致

证据：[main.rs:1316-1324](../rust/src/main.rs#L1316-L1324) `run` 未给 `--dry-run/--live` 时跟随 `runtime.mode`；[main.rs:1158-1162](../rust/src/main.rs#L1158-L1162) `task create` 未给 `--live` 时固定生成 `run --dry-run`。

后果：配置 `runtime.mode = "live"` 的用户执行 `task create` 会得到一个静默的 dry-run 任务；新人无法预测哪个命令遵循配置。

修复：共享一个 mode resolver；`task create` 要求显式 `--dry-run|--live`（或 `--mode`），并在输出中打印解析后的 mode。

### P2-3 `run --dry-run --confirm` 被接受

证据：[main.rs:199-204](../rust/src/main.rs#L199-L204)：`--dry-run` 只与 `--live` 冲突，`--confirm` 只要求 `--live`；因此 `--dry-run --confirm` 通过 clap，`--confirm` 被静默忽略。[AI_ONBOARDING.md:345](../docs/AI_ONBOARDING.md#L345) 却声称二者冲突。

后果：自动化/检查清单以为确认门禁被执行，实际没有。

修复：给 `confirm` 加 `conflicts_with = "dry_run"`，或改成显式 `--mode` 枚举；加解析测试。

### P2-4 运行期参数错误返回 exit 4，与 clap 用法错误 exit 2 混用

证据：[main.rs:1431-1433](../rust/src/main.rs#L1431-L1433)、[main.rs:1153-1155](../rust/src/main.rs#L1153-L1155)、[main.rs:691-693](../rust/src/main.rs#L691-L693) 都是运行期 `bail`；[main.rs:219-224](../rust/src/main.rs#L219-L224) 把所有 `Err` 映射为 `ConfigFailure=4`；clap 用法错误退出 2，而 `DeviceFailure` 也是 2。

后果：脚本无法区分布局错误、配置错误、设备错误；新人看到 exit 4 会去修配置，实际可能是 LDPlayer 没启动。

修复：所有可静态判定的约束下沉到 clap（exit 2）；设备/ADB 错误在命令边界映射到 `DeviceFailure`；文档化退出码表。

### P2-5 `--all-instances` 帮助与语义不符

证据：[main.rs:176](../rust/src/main.rs#L176)、[main.rs:205](../rust/src/main.rs#L205) 写“所有已发现实例”；实际 `run_all_instances` 使用 `store.config.emulator.effective_instances()`（[main.rs:1437](../rust/src/main.rs#L1437)），只跑配置中 enabled 的实例。

后果：机器上有 3 个模拟器但配置只启用 1 个时，用户以为全部签到完成。

修复：帮助改为“配置中 enabled 的实例（禁用项跳过）”；启动前打印计划运行的实例列表；若确实要“发现即运行”，另设 `--discover-all`。

### P2-6 变更型命令的确认门槛不一致

证据：

- `profile sync` 需要 `--confirm`（[main.rs:691-693](../rust/src/main.rs#L691-L693)）；
- `task create --live` 需要 `--confirm`（[main.rs:174](../rust/src/main.rs#L174)、[main.rs:1153](../rust/src/main.rs#L1153)）；
- `app install` 没有 `--confirm`，且可能自动启动模拟器、写 `app.apk_path`（[main.rs:1056-1069](../rust/src/main.rs#L1056-L1069)）；
- `task delete` 永远 `/F` 强制删除，没有确认（[main.rs:1195-1207](../rust/src/main.rs#L1195-L1207)）。

后果：新人可以一条命令替换 APK、改配置、强删计划任务，与“guard everything destructive”的项目风格不一致。

修复：`app install`、`task delete` 增加 `--confirm`（或 `--force`）；`task create` 覆盖已有任务时要求确认；help 中写明 auto-launch、config 写入、`/F` 覆盖等副作用。

### P2-7 无子命令静默启动 GUI，help 未说明

证据：[main.rs:231](../rust/src/main.rs#L231) `unwrap_or(CommandKind::Gui)`；顶层 about 没有 after_help。

后果：脚本或只想看帮助的用户敲 `zhfd-checkin.exe` 会弹 GUI；headless 环境可能挂起。

修复：顶层 `after_help` 明确“无子命令 = 启动 GUI；脚本必须显式传命令”；考虑 `arg_required_else_help`。

### P2-8 命令语法混用动词/名词，单子命令分组增加深度

证据：[main.rs:86-123](../rust/src/main.rs#L86-L123) 同时有动词 `diagnose/report/run/vision` 和名词组 `profile/app/config/instance/task/ui`；`app`、`instance`、`ui` 各只有一个叶子命令。

后果：新人无法预测动作属于 `run`、`profile sync`、`instance list` 还是 `vision --image`；`report` 与 `diagnose` 语义接近。

修复：只保留真正多操作的命名空间（`profile`、`task`、`config`），单操作提升为动词（`instances`、`install-app`、`dump-ui`、`analyze-image`）；旧名用 alias 保留一个版本。

### P2-9 GUI 直接依赖 clap 类型，CLI 不是薄适配层

证据：

- [gui.rs:3-8](../rust/src/gui.rs#L3-L8) 直接 import `RunArgs`、`TaskCommand`、`InstanceTargetArgs`、`ConfigCommand`；
- [gui.rs:367-373](../rust/src/gui.rs#L367-L373) 手工构造 `RunArgs`；
- [gui.rs:1716-1776](../rust/src/gui.rs#L1716-L1776) 手工构造 `TaskCommand`；
- [main.rs:1995-2009](../rust/src/main.rs#L1995-L2009) `run_inner_with_device` 接收 `&RunArgs`，`args.confirm` 进入 `guarded_tap`（[main.rs:2644](../rust/src/main.rs#L2644)）。

后果：改一个 flag 会同时改 GUI 和计划任务字符串；安全关键的 tap 授权依赖 CLI wire struct；“用户输入 --confirm”和“GUI 确认框已接受”被压成一个 bool。

修复：引入与 clap 无关的 `RunRequest` / `RunPolicy { mode, explicit_confirmation, source }` / `TaskSpec` / `InstanceSelector`；CLI 只做 `From<RunArgs>` 适配，GUI 直接构造请求；`guarded_tap` 从 policy 取确认。

### P2-10 handler 混合业务、stdout 和退出码

证据：`diagnose`、`sync_profile`、`task_command`、`run`、`instance_command`、`config_command` 都在 handler 内直接 `println!` 并返回 `ExitCode`；GUI 调用同一 handler 但看不到 stdout（GUI 是 `windows_subsystem = "windows"`），只能显示“完成，退出码”。

后果：GUI 无法展示 diagnose/task 的成功输出；输出格式与业务逻辑耦合；无法统一做脱敏/JSON。

修复：handler 返回 typed report（`DiagnoseReport`、`TaskOutcome`、`RunOutcome`），CLI render 层负责文本/JSON 和退出码；GUI 渲染同一 report。

### P2-11 `task_command` 重复加载配置、签名不一致

证据：[main.rs:239-244](../rust/src/main.rs#L239-L244) 已加载 store，但只把 `config_path` 传给 task 命令；`TaskCommand::Create` 在 [main.rs:1167](../rust/src/main.rs#L1167) 又 `load_store_for_cli` 一次；GUI 必须传 `Some(self.store.path)`。

后果：配置解析/默认创建/备份副作用可能出现两次；`task query/delete` 也被配置加载门禁挡住。

修复：`task_create(store, spec)`、`task_query(name)`、`task_delete(name)`；CLI dispatch 按配置需求分类。

### P2-12 GUI 二进制有第二套手写参数解析

证据：[main.rs:505-517](../rust/src/main.rs#L505-L517) `launch_gui` 扫描 `args_os`，只识别 `about`、`--about`、精确 `--config`；不识别 `--config=PATH`、`--help`、`--version`。

后果：`zhfd-checkin-gui.exe --config=C:\x\config.toml` 静默打开默认配置；CLI/GUI 语法不一致。

修复：GUI 入口复用同一个 clap parser 或 `GuiLaunchOptions`；删除手写扫描；补测试。

### P2-13 CLI 解析/分发测试很少

证据：只有 `--config` 位置、损坏配置、task command line 等少数测试；没有覆盖 `--dry-run` vs `--live`、`--confirm` requires/conflicts、`task create` 组合、无子命令默认 GUI、`CommandKind → handler` 映射。

后果：改 flag 关系或新增子命令可能通过 CI 但破坏脚本。

修复：表驱动 `Cli::try_parse_from` accept/reject 测试；`--help` 快照；退出码测试；login-only 加入后同样覆盖。

### P3

- `vision` 仍要求有效配置（[main.rs:239-246](../rust/src/main.rs#L239-L246)），应移到配置加载之前或 `debug image`。
- `config show` 已默认脱敏（[main.rs:625-641](../rust/src/main.rs#L625-L641)），但文档未说明；`ui dump` 可能含 PII，help 应标注。
- `about` 固定打印“EXE 所在目录”（[main.rs:600](../rust/src/main.rs#L600)），与 recorded/LocalAppData 优先级不符；建议新增 `config path`。
- `--serial` help 写“覆盖 index 推导值”，实际是“必须与 index 一致，否则拒绝”（[main.rs:325-334](../rust/src/main.rs#L325-L334)）。
- `task create/delete` 硬编码 `/SC DAILY`、`/ST config.window.start`、`/F`，无 `--at/--schedule/--force`。
- 任务名默认值在 create/delete/query 重复三处。
- 帮助中英混排（Usage/Commands/Options 英文，参数说明中文）。
- OPERATIONS 仍写 Cargo 0.3.2。

---

## 4. 正面结论

- 最大嵌套只有 2 层，命令数量 12 个，不存在深层命令树。
- `InstanceTargetArgs` 在 7 个设备命令中复用，`--instance-index/--serial` 语义统一。
- `--config` 是真正的全局参数，前后位置都有测试。
- `about`/`gui` 在配置加载前分派，损坏配置仍能进 GUI Repair。
- clap 层已有 fail-closed 基础：`--dry-run` 与 `--live` 互斥，`--confirm` requires `--live`，裸 `run` 缺 confirm 不点击。
- `run --all-instances` 有共享 preflight、逐实例 result 和 summary，单实例状态机未被污染。
- `profile sync` 是变更型命令的范本：显式 `--confirm`、LDPlayer 配置备份、改后几何验证、失败回滚。
- 计划任务命令把解析后的绝对 `--config` 路径 pin 进 `/TR`，避免工作目录漂移。
- 退出码有单一枚举（0–5），`result.json` 是稳定产物。

---

## 5. 目标 CLI（Design B：面向操作者）

```text
zhfd-checkin [--config PATH] <command>

只读 / 诊断
  doctor [target] [--json]          # alias: diagnose；不创建配置，打印下一步
  report [--json]                   # 生成默认脱敏诊断报告
  instances [--json]                # alias: instance list
  profiles list | detect [target]
  config path | show | open         # path 输出 resolved path/origin；show 默认脱敏
  debug dump-ui [target]            # alias: ui dump；help 标注可能含 PII
  debug image --image PATH          # alias: vision；无需配置

安全演练 / 运行
  run [--dry-run | --live --confirm] [--all-instances] [target]
  login [--confirm-login] [target]  # 新增；无 confirm 时只观察、零 tap（ADR-0005）

危险操作（help 前缀 [危险]，执行前打印一行警告）
  install-app --apk PATH [target]   # alias: app install；新增 --confirm
  profile sync --id ID --confirm
  task create | delete | query      # delete 新增 --confirm；create 打印计划
```

`target` = `--instance-index N | --serial S`（clap ArgGroup，互斥；`--all-instances` 与二者互斥）。

配套规则：

1. 每个命令/flag 有中文 about/help；顶层 `after_help` 给出三条首次使用命令、无子命令启动 GUI 的说明、以及安全契约。
2. `help_heading` 分组：只读 / 安全演练 / 危险；危险命令 about 前缀 `[危险]`。
3. `run` 默认输出一行 `status/mode/route/error_code` + `下一步: ...`；完整 JSON 通过 `--json`。
4. `config path` 输出 resolved path、origin、created_default；`about` 不再硬编码 EXE 目录。
5. `doctor` 不落盘配置；其他命令若发现 `created_default`，打印配置路径并提示先走 GUI Setup。
6. `task create` 要求显式 mode，打印最终计划任务命令行；`--all-instances` 帮助改为“配置中 enabled 的实例”。
7. 旧命令名保留至少一个版本的 `visible_alias`，不破坏现有脚本。

---

## 6. 迁移计划

### Phase 1（低风险，先做）

- 修 P1-1：`--all-instances` 与 target 在 clap 层互斥（run + task create）。
- 修 P2-3：`--confirm` 与 `--dry-run` 互斥。
- 补全所有命令/flag 的 help + 顶层 after_help + 分组。
- 配置按命令懒加载；`vision`、`profile list`、`task query/delete` 不再依赖配置。
- 新增 `config path`；`about` 改为打印解析优先级。
- 增加 `Cli::try_parse_from` 表驱动测试、`--help` 快照、退出码测试。
- 不重命名任何命令。

### Phase 2（命令面收敛）

- 新增 `doctor`、`instances`、`install-app`、`debug dump-ui`、`debug image`，旧名保留 alias。
- 新增 `login` 一级命令（ADR-0005）；不要塞进 `RunArgs` 或 `task create`。
- `task create/delete` 增加确认；`app install` 增加 `--confirm`。
- `run` 增加人类可读摘要 + `--json`。

### Phase 3（分层，较大）

- 引入 `commands/` typed request/report 层；CLI 只做 `From<RunArgs>` 适配；GUI 直接构造请求。
- `RunPolicy { mode, explicit_confirmation, source }` 取代 `&RunArgs` 进入状态机；`guarded_tap` 从 policy 取确认。
- 把 parser/dispatch 移到 `cli/`，GUI 移入 GUI 二进制或 feature gate；`main.rs` 只保留入口。
- 每一步保持 `guarded_tap`/`authorize_*` 门禁顺序不变。

---

## 7. 风险

- 改名/别名会触及脚本和计划任务字符串；Phase 1 不做重命名。
- 把 `--confirm` 从 `RunArgs` 移到 `RunPolicy` 会触及安全关键路径；必须用现有反例测试和新的 policy 测试保护。
- `task create` 增加必填 mode 会改变现有自动化；建议先警告/打印，下一版再强制。
- GUI 与 CLI 共用 handler 是当前优点；分层时不要复制业务逻辑。
- `login` 会新增设备 tap 路径；必须复用 `authorize_auto_login` 门禁并写 ADR-0005，不能绕过 `guarded_tap` 的签到门禁。

---

## 8. 审计边界

- 本报告是 AI 辅助设计审计，**不是独立验收**；未修改 CLI 行为。
- 行号以审计时工作树为准，函数名比行号稳定。
- 未覆盖 GUI 黑盒交互、真实设备上的命令组合、发布资产中的 CLI 行为。
- 未评估 helper/补全（shell completion）、本地化框架等非核心项。

---

## 9. Remediation（2026-10-21，Phase 1 已实现）

| 发现 | 状态 | 变更 |
|---|---|---|
| P1-1 计划任务组合 | **已修复** | `run` 与 `task create` 的 `--all-instances` 在 clap 层与 `--instance-index`/`--serial` 互斥；解析测试覆盖 |
| P1-2 help 不可发现 | **已修复** | 所有一级命令/子命令/关键 flag 补中文 help；顶层 `after_help` 写首次使用路径、无子命令启动 GUI、live/dry-run 安全契约 |
| P2-3 `--dry-run --confirm` | **已修复** | `--confirm` 与 `--dry-run` 互斥；`--live` 与 `--confirm` 互相 requires（clap exit 2） |
| P2-4 运行期参数错误 | **部分修复** | `run --live`、`task create --live`、`profile sync --id` 缺少确认时改为 clap exit 2；设备/ADB 错误仍统一 exit 4（未改） |
| P2-1 配置预加载 | **已修复（Phase 1 范围）** | `vision`、`profile list`、`task query/delete` 不再加载配置；新增 `config path`，用 `ConfigStore::resolve_path` 在**不解析、不创建**配置的情况下输出 resolved path/root/origin/exists，因此损坏配置时也可用 |
| P2-5 `--all-instances` 帮助 | **已修复** | 帮助改为“配置中 enabled 的实例（不能与实例选择器组合）” |
| P2-13 CLI 测试 | **部分修复** | 新增 mode/confirm/批处理互斥、profile sync confirm、help about、config path、损坏配置下 profile list 的解析/行为测试 |
| P2-2/P2-6/P2-7/P2-8/P2-9/P2-10/P2-11/P2-12、P3 | **未修复** | 留待 Phase 2/3：mode 默认统一、app install/task delete 确认、命令改名、typed command 层、handler 输出拆分、GUI clap 解耦、退出码分类 |

> 本节是审计后的 remediation 记录，不改变上文的时点结论。Phase 1 未修改 `guarded_tap` / `authorize_*` 门禁顺序。
