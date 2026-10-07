# Rust 迁移状态（2026-09-25）

> 当前运行默认签到窗口已统一为 **21:00–23:59（Asia/Shanghai）**。下文早期验证记录中的 21:30 是当时的历史配置，不代表当前默认值。

## 当前交付范围

Rust 第一阶段 CLI 已建立，Python 仍是行为参考基线。当前 CLI 使用：

- `image` + 手写 HSV、颜色覆盖率、文字行投影和连通区域搜索；
- ADB 命令实现截图、启动 App、点击、滑动、hierarchy dump、APK 安装；
- LDPlayer `ldconsole.exe` 自动发现、实例读取、启动和 profile 同步入口；
- `egui/eframe` 基础 GUI；
- `config.toml`、`logs/`、`reports/`、`captures/` 均位于 EXE 所在目录。

当前没有引入 `purecv`、`scirs2-vision`、`ndarray` 或 `numrs2`。现有四态算法不需要完整 OpenCV；后续只有在 profile 标定证明手写算法不足时才评估这些库。

## 命令

```powershell
zhfd-checkin.exe                 # GUI
zhfd-checkin.exe about
zhfd-checkin.exe diagnose
zhfd-checkin.exe report
zhfd-checkin.exe profile list
zhfd-checkin.exe profile detect
zhfd-checkin.exe profile sync --id portrait_900x1600_d240 --confirm
zhfd-checkin.exe ui dump
zhfd-checkin.exe vision --image .\button.png
zhfd-checkin.exe run --dry-run
zhfd-checkin.exe run --live --confirm
zhfd-checkin.exe app install --apk D:\Downloads\zhfd.apk
zhfd-checkin.exe task create
zhfd-checkin.exe task query
zhfd-checkin.exe task delete
```

`profile sync` 会备份已发现的 LDPlayer 配置，修改目标实例分辨率/DPI，并重启实例；没有实现也不会修改 model、IMEI、账号、Cookie、Token 或定位数据。

## Profile

必须的八个雷电预设均已内置：

```text
landscape_1600x900_d240
portrait_900x1600_d240
landscape_1280x720_d240
portrait_720x1280_d240
landscape_1920x1080_d280
portrait_1080x1920_d280
landscape_960x540_d160
portrait_540x960_d160
```

工作区当前已有的 LDPlayer 实例实际报告为 `900x1600@320`，因此额外保留了兼容 profile `portrait_900x1600_d320`。随机 model、IMEI 和 ADB serial 不参与 profile 匹配。

只有已标定 profile 才允许 `--live`；当前 `portrait_900x1600_d240` 和兼容的 `portrait_900x1600_d320` 已标定，其余 Profile 仍只允许检测和 dry-run。当前三个生产实例均为 `portrait_900x1600_d320`。

## 已验证

```text
cargo check       PASS
cargo test        PASS
Python 四态回归    PASS（保留在原项目）
Rust fixture 四态  PASS
Rust ready crop    ready（4309 白字像素，双行结构）
Rust success crop  success（H≈61）
Rust release build PASS
```

当前 release CLI/GUI EXE 在本机生成于：

```text
rust/target/release/zhfd-checkin.exe
```

## 尚未完成

- 其余七个 profile 的 `locating -> ready` 实态和最终 live 能力验证仍未完成；当前八个均已完成灰态截图、坐标测量和窗口外 dry-run，`portrait_900x1600_d240` 已完成一次真实窗口 dry-run 和一次 guarded live 验证。
- UiAutomator Rust crate 的独立封装验证。目前使用 ADB 的 `uiautomator dump` 作为兼容方案；
- 干净 Windows 环境上的便携 EXE 验收。

## 2026-09-25 `portrait_900x1600_d240` 首轮实机验证

已完成以下操作：

- `profile sync --id portrait_900x1600_d240 --confirm` 首次写入配置后，旧实现因 LDPlayer 停止/启动竞态报告启动超时；随后修复为等待实例明确停止后再启动。
- 修复后的同步命令于 2026-09-25 17:43 再次执行成功，并生成 LDPlayer 配置备份。
- `diagnose` 和 `profile detect` 均识别为 `900x1600@240`、`portrait_900x1600_d240`。
- 实机截图测得 240 DPI 下灰色签到圆约为 `(449,778)`、`232x232`；已更新该 profile 的 dry-run 回退区域。该节记录时仍保持 `calibrated=false`，后续已在真实窗口完成标定。
- 修复页面探测：灰色“无法签到”按钮没有饱和色，动态彩色按钮检测会失败；现在会先用 profile 回退区域分类灰色状态，避免误把已在签到页的页面当成首页。

17:46 的 release dry-run（临时将轮询/超时缩短，运行后已恢复正式配置）证据：

```text
profile_id: portrait_900x1600_d240
state_history: [gray, gray, gray, gray]
button: (449, 778, 232, 232)
clicked: false
dry_run_ready: false
error: location_timeout
```

对应日志目录：`rust/target/release/logs/2026-09-25_174604/`。这次结果符合安全预期：没有点击；由于当前本机时间为 17:46，早于配置窗口 21:30-23:59，页面保持灰色，尚不能据此完成 `locating -> ready` 的时间窗口验证。

本轮不等待签到窗口：窗口外的 profile 几何、导航、WebView 滚动和灰态分类能力已经先行验证。剩余的 `locating -> ready` 及正式点击门禁，仍保留到真实窗口内做最终确认；在此之前不得把 profile 标记为 `calibrated=true`。

## 2026-09-25 四个 240 DPI profile 的窗口外能力验证

按计划不等待签到窗口，先完成不触发签到的能力验证：profile 同步、ADB 重连、服务页导航、WebView 滚动、灰态按钮定位和状态分类。四个 profile 均保持 `calibrated=false`，因此不会允许 live 点击。

| Profile | 实测按钮区域 | dry-run 状态 | 日志目录 |
|---|---:|---|---|
| `portrait_900x1600_d240` | `(449,778) 232x232` | `gray`，未点击 | `logs/2026-09-25_182849/` |
| `landscape_1600x900_d240` | `(800,779) 232x232` | `gray`，未点击 | `logs/2026-09-25_182945/` |
| `landscape_1280x720_d240` | `(640,409) 256x256` | `gray`，未点击 | `logs/2026-09-25_182556/` |
| `portrait_720x1280_d240` | `(360,852) 256x256` | `gray`，未点击 | `logs/2026-09-25_182754/` |

其中 landscape 页面新增了兼容导航：点击“业务”标签、向上滚动服务目录直到发现“晚点名签到”，进入页面后再滚动 WebView 使签到圆进入视口。Rust CLI 的 release dry-run 已验证该路径，输出示例：

```text
state_history: [gray, gray, gray, gray]
clicked: false
dry_run_ready: false
error: location_timeout
```

这里的 `location_timeout` 是测试时临时将轮询/超时缩短后的收尾结果，不是设备或视觉失败；每次测试后正式配置均已恢复。由于当前时间早于应用的 `21:30-23:59` 窗口，四个页面都显示“无法签到”。本轮不修改模拟器系统时间，也不绕过应用的时间/定位校验。

已新增的保护/兼容逻辑：

- `profile sync` 等待 LDPlayer 真正停止后再启动，避免 ADB offline 和启动竞态；
- 灰态页面先用 profile-specific fallback 分类，不再误导航；
- landscape 页面支持“业务”入口和服务目录滚动；
- 进入签到页后允许安全滚动，不会触发签到点击；
- live 门禁仍要求 `--live --confirm`、已标定 profile、前台包名、时间窗口和连续 ready 帧。

## 2026-09-25 八个 profile 的几何验证完成

继续不等待签到窗口，已完成剩余四个 profile 的同步、导航、灰态按钮测量和 release dry-run：

| Profile | 实测按钮区域 | dry-run 状态 | 日志目录 |
|---|---:|---|---|
| `landscape_1920x1080_d280` | `(960,907) 307x308` | `gray`，未点击 | `logs/2026-09-25_184249/` |
| `portrait_1080x1920_d280` | `(540,907) 308x307` | `gray`，未点击 | `logs/2026-09-25_184336/` |
| `landscape_960x540_d160` | `(480,352) 177x177` | `gray`，未点击 | `logs/2026-09-25_184412/` |
| `portrait_540x960_d160` | `(270,525) 177x177` | `gray`，未点击 | `logs/2026-09-25_184500/` |

至此八个要求 profile 都已经完成：

- 分辨率/DPI 同步和 ADB 重连；
- 页面入口导航（含 landscape 的“业务”标签和服务目录滚动）；
- 灰态签到圆的 profile-specific fallback 坐标；
- Rust release dry-run 的灰态分类；
- `clicked=false` 安全确认。

该节记录时八个 profile 仍全部保持 `calibrated=false`。随后仅 `portrait_900x1600_d240` 通过真实窗口验证并开放 guarded live；其余七个仍保持 `calibrated=false`。

## 2026-09-25 便携 EXE staging 验收

新增 `scripts/portable_smoke.ps1`，用于在 Windows 临时目录模拟便携部署。脚本只复制：

- `rust/target/release/zhfd-checkin.exe`；
- `autojs6/fixtures/ready.png`。

随后从 staging 目录执行：

```powershell
.\scripts\portable_smoke.ps1
```

验收内容：

- `about` 能启动并打印版本信息；
- `profile list` 包含八个要求 profile；
- `config show` 在 EXE 同目录创建 `config.toml`；
- `vision --image fixtures\ready.png` 返回 `state=ready`；
- 相对 fixture 路径从 staging 工作目录解析，不依赖仓库当前目录。

2026-09-25 本机结果：

```text
PORTABLE_SMOKE_PASS
profile_count=8+
vision_state=ready
```

CI 已加入同一 smoke test。该结果证明“单 EXE + 外部 fixture”便携路径通过；尚未声称完成真正干净 Windows 镜像验收，因为当前验证仍运行在开发机 Windows 环境。最终便携验收仍需在没有 Rust、Python、Node、Android SDK 和 LDPlayer 的干净 Windows 环境中执行 `about`、`profile list`、`config show` 与 fixture 视觉命令。

## 2026-09-25 GUI 诊断结果完善

GUI 诊断页现已支持：

- 导出 JSON 诊断报告（`reports/diagnostic-*.json`）；
- 刷新并展示最近一次 `result.json`；
- 展示 mode、profile、state history、serial、clicked、success；
- 将 `unknown_state`、`location_timeout`、`outside_window`、ADB 不可用等错误映射为用户可读提示；
- dry-run 异步任务完成后自动刷新最近运行结果。

新增 CLI 命令：

```powershell
zhfd-checkin.exe report
```

报告包含配置和最近运行结果，但会清空通知 URL 等不适合分享的字段；不包含 Token、Cookie 或定位地址。

## 2026-09-25 GUI 后台任务与同步确认

GUI 已进一步将可能阻塞模拟器或 ADB 的操作放入后台任务：

- dry-run；
- LDPlayer / ADB 环境诊断；
- 当前 Profile 检测；
- Profile 同步、配置备份和模拟器重启；
- APK 安装/更新。

底部状态栏会显示正在运行的任务，后台任务结束后会统一更新状态并刷新最近一次运行结果。Profile 同步现在使用独立确认窗口，明确提示会修改分辨率/DPI 并重启实例；取消不会触发任何模拟器操作。

这一步仍不开放 GUI live 点击；正式签到继续要求 CLI 的 `run --live --confirm` 以及既有 profile、时间窗口和连续 ready 门禁。

## 2026-09-26 `portrait_900x1600_d240` 首次真实点击验证

在 **2026-09-26 21:40 至 21:43（Asia/Shanghai）**，先执行窗口内 dry-run：

```text
profile_id: portrait_900x1600_d240
state_history: [locating, locating, locating, ready, ready]
clicked: false
dry_run_ready: true
error: null
```

随后仅为该 profile 开启 `calibrated=true`，重新构建 release EXE，并在 **2026-09-26 22:07:33（Asia/Shanghai）** 执行：

```powershell
zhfd-checkin.exe run --live --confirm
```

结果：

```text
profile_id: portrait_900x1600_d240
state_history: [ready, ready, success]
clicked: true
success: true
dry_run_ready: false
error: null
exit_code: 0
```

点击坐标为 `(448,779)`，点击后识别到 success 状态。完整日志目录：

```text
rust/target/release/logs/2026-09-26_220733/
```

该验证只开放了 `portrait_900x1600_d240`。其余七个要求 profile 仍保持 `calibrated=false`，不能执行 live 点击。该次结果证明该 profile 已完成一轮真实签到窗口内的状态、坐标、点击和成功态闭环，但不代表其他 profile 已完成验证。

## 2026-09-26 运行安全边界与结构化失败日志

本轮继续固化单 Profile 实证后的运行安全能力：

- 运行轮询在当前签到窗口内会同时受 `location_timeout_seconds` 和窗口结束时间限制；
- 正常窗口与跨午夜窗口都使用显式时区计算窗口剩余时间；
- 点击后允许继续等待 success，不会因为窗口刚结束而丢失点击结果；
- `run` 的设备、视觉、Profile 和 ADB 异常会统一收敛到 `result.json`，并记录 `mode`、`serial`、`profile_id`、状态历史、点击和错误；
- 未标定 Profile 的 live 请求记录为 `uncalibrated_profile`，不会直接绕过日志；
- 点击动作经过 `guarded_tap`，只有最终确认的 `ready`、显式 live 确认、已标定 Profile、目标前台包名、有效时间窗口和连续 ready 帧全部满足时才会调用设备 tap；
- 新增 Mock Device 测试，覆盖 gray、locating、unknown、一帧 ready、未标定、前台包名不匹配、窗口外和正确坐标点击。

本轮验证：

```text
cargo fmt --check        PASS
cargo clippy -D warnings PASS
cargo test               PASS（24 tests）
```

## 2026-09-26 Profile 同步事务复核增强

Profile 同步已补充事务性校验和结构化日志：

- 同步前记录 LDPlayer `list2` 参数；实例运行时优先记录 ADB `wm size`、`wm density` 的实际参数；
- 修改分辨率/DPI 后，实例重启并重新连接 ADB；
- 同步后重新读取实际宽度、高度和 DPI，并必须与目标 Profile 精确一致；
- 如果实例原本未运行，则使用同步后的 LDPlayer 实例参数完成非运行态复核，不主动改变原有运行状态；
- 修改成功但停止、等待、启动或复核失败时，自动尝试停止实例、恢复备份并恢复原有运行状态；
- 同步日志写入 `logs/sync-YYYY-MM-DD_HHMMSS/`，包括 `events.jsonl` 和 `sync.json`；
- 日志记录同步前参数、目标参数、同步后参数、备份路径、是否尝试修改、是否修改成功、是否重启、回滚是否成功和错误原因。

同步命令成功的必要条件现在是：

```text
实际 width/height/dpi == 目标 Profile width/height/dpi
```

本轮实施时本机本地时钟已经晚于配置的 `21:30-23:59` 窗口，因此没有在窗口外切换 LDPlayer 到横屏 Profile，也没有伪造 `landscape_1600x900_d240` 的真实窗口 dry-run 结果。下一次窗口内继续执行：

```powershell
zhfd-checkin.exe profile sync --id landscape_1600x900_d240 --confirm
zhfd-checkin.exe profile detect
zhfd-checkin.exe run --dry-run
```

只有当日志同时确认 `profile_id=landscape_1600x900_d240`、连续 ready、`dry_run_ready=true`、`clicked=false` 且 `error=null` 时，才评估是否为该横屏 Profile 开放一次 guarded live 验证。

## 2026-09-27 跳过真实窗口后的离线交付推进

本轮按计划暂不切换 LDPlayer Profile，也不执行真实签到窗口内的横屏验证，转而完成 GUI 和日志管理能力：

- GUI 新增“计划任务”页面；
- GUI 新增“运行日志”页面；
- 计划任务的查询、创建、删除均使用后台任务，不阻塞界面；
- GUI 默认只创建 dry-run 计划任务；live 计划任务需要勾选后再次确认；
- GUI 不提供直接 live 签到按钮，live 计划任务仍调用 CLI 的 `run --live --confirm`，继续受到 Profile、前台包名、时间窗口和 ready 门禁保护；
- 日志页面同时展示普通运行日志和 Profile 同步日志；
- 普通运行日志从 `result.json` 提取状态，Profile 同步日志从 `sync.json` 提取同步状态和回滚信息；
- 新增日志列表解析测试，确认运行日志和同步日志可以混合排序展示。

本轮没有执行：

```powershell
zhfd-checkin.exe profile sync --id landscape_1600x900_d240 --confirm
zhfd-checkin.exe run --dry-run
```

因此 `landscape_1600x900_d240` 仍保持 `calibrated=false`，没有把离线构建或旧的窗口外结果误记为真实窗口验证。

## 2026-09-27 GUI 应用管理、构建元数据与离线恢复测试

本轮继续跳过真实签到窗口，完成以下离线交付项：

### APK 应用管理

- GUI 的“安装/更新选中的 APK”现在先打开确认窗口，不会在按钮点击瞬间修改设备；
- 确认窗口展示 APK 路径、目标包名，并明确提示会执行 `adb install -r`；
- 安装放入后台任务，完成后展示安装输出、目标包名和安装后版本；
- GUI 新增“刷新版本”，通过当前 ADB 设备读取目标包的 `versionName`；
- 安装结果和当前已安装版本会保留在应用管理页。

### About 与构建元数据

- 新增 `rust/build.rs`，构建时写入 Git short commit；
- CLI `about` 和 GUI About 页面展示版本、Git commit、支持的 Profile 和主要依赖；
- 依赖说明明确区分 Rust 内置视觉实现、GUI 依赖以及外部 ADB/LDPlayer 依赖。

### 便携 EXE 目录验收

`ConfigStore::load` 现在会确保 EXE 同目录存在：

```text
logs/
captures/
reports/
```

portable smoke test 现在同时验证：

- `config.toml`；
- `logs/`；
- `captures/`；
- `reports/`；
- About 元数据；
- 8 个 Profile；
- fixture 视觉识别。

### Profile 同步恢复离线测试

新增 `SyncLifecycle` 抽象和 Mock 实现，离线覆盖：

- 原配置文件被修改后，恢复备份并恢复原本运行状态；
- 恢复配置成功但重启失败时，仍保留恢复后的配置并报告重启错误；
- 停止和启动调用次数符合预期。

本轮没有切换 `landscape_1600x900_d240`，也没有执行真实窗口 dry-run 或 live 点击。

## 2026-09-30 双 LDPlayer 实例支持

配置现在兼容两种形式：

### 旧版单实例配置

继续支持原有字段：

```toml
[emulator]
instance_index = 0
serial = ""
```

如果没有配置 `instances`，程序会把上述字段转换为一个默认实例，不影响已有用户配置。

### 双实例配置

需要同时对接两个模拟器时，在 `config.toml` 中使用：

```toml
[emulator]
ldconsole_path = ""
adb_path = ""
instance_index = 0
serial = ""
auto_launch = true

[[emulator.instances]]
name = "签到实例 A"
instance_index = 0
serial = ""
enabled = true

[[emulator.instances]]
name = "签到实例 B"
instance_index = 1
serial = ""
enabled = true
```

支持的 CLI 入口：

```powershell
zhfd-checkin.exe instance list
zhfd-checkin.exe diagnose --instance-index 0
zhfd-checkin.exe diagnose --instance-index 1
zhfd-checkin.exe profile detect --instance-index 0
zhfd-checkin.exe profile detect --instance-index 1
zhfd-checkin.exe run --dry-run --instance-index 0
zhfd-checkin.exe run --dry-run --instance-index 1
zhfd-checkin.exe run --dry-run --all-instances
zhfd-checkin.exe ui dump --instance-index 1
zhfd-checkin.exe app install --apk D:\Downloads\zhfd.apk --instance-index 1
```

也可以使用显式 ADB serial：

```powershell
zhfd-checkin.exe run --dry-run --serial 127.0.0.1:5557
```

其中 `--serial` 优先于实例配置；未指定时按 LDPlayer 实例 index 使用默认 serial：

```text
index 0 -> 127.0.0.1:5555
index 1 -> 127.0.0.1:5557
```

`--all-instances` 会依次运行每个 LDPlayer 实例，并为每个实例生成独立的 `logs/<timestamp>/result.json`。实例的 index、名称和 serial 会记录在运行结果中。Profile、点击门禁和 live 安全策略仍然按每个实例独立执行；不会因为其中一个实例已标定而放开另一个实例。

GUI 首页、Profile、应用管理和运行任务会使用当前选择的实例；计划任务可以选择只运行当前实例或依次运行全部实例。当前版本没有把两个实例合并成一个共享设备状态，避免串用 ADB serial、Profile 或点击结果。

## 2026-09-30 双实例 dry-run 与日志隔离验证

在 **2026-09-30 15:31（Asia/Shanghai）**，对两个已配置的 LDPlayer 实例分别执行了：

```powershell
zhfd-checkin.exe run --dry-run --instance-index 0
zhfd-checkin.exe run --dry-run --instance-index 1
```

由于当时尚未进入配置的 `21:30-23:59` 签到窗口，新的窗口保护在连接设备并识别 Profile 后立即返回 `outside_window`，没有继续截图轮询，也没有点击。这是预期的离线/窗口外安全结果。

实例 0：

```json
{
  "instance_index": 0,
  "instance_name": "签到实例 0",
  "serial": "127.0.0.1:5555",
  "profile_id": "portrait_900x1600_d320",
  "clicked": false,
  "success": false,
  "error": "outside_window"
}
```

日志目录：

```text
rust/target/release/logs/2026-09-30_153114/
```

实例 1：

```json
{
  "instance_index": 1,
  "instance_name": "签到实例 1",
  "serial": "127.0.0.1:5557",
  "profile_id": "portrait_900x1600_d320",
  "clicked": false,
  "success": false,
  "error": "outside_window"
}
```

日志目录：

```text
rust/target/release/logs/2026-09-30_153116/
```

隔离检查通过：

- 两个 `result.json` 的 `instance_index`、实例名和 serial 分别正确；
- 两个日志目录不同；
- 两个实例都独立识别为 `portrait_900x1600_d320`；
- 两个实例均 `clicked=false`；
- ADB serial 未串用。

本次验证确认了多实例选择和结构化日志隔离，但没有验证签到窗口内的 `locating -> ready`，也没有改变任何 Profile 的 calibrated 状态。

## 2026-09-30 双实例窗口内真实验证

在 **2026-09-30 22:05–23:12（Asia/Shanghai）** 配置签到窗口内，先对两个实例分别执行 release dry-run：

### 实例 0 dry-run

```text
instance_index: 0
instance_name: 签到实例 0
serial: 127.0.0.1:5555
profile_id: portrait_900x1600_d320
state_history: [locating, locating, ready, ready]
dry_run_ready: true
clicked: false
error: null
```

日志目录：

```text
rust/target/release/logs/2026-09-30_230010/
```

### 实例 1 dry-run

首次窗口内执行时，实例 1 仍停留在首页，视觉轮询结果为 `locating`，最终 `location_timeout`。随后修复导航逻辑：只有可点击的 `晚点名签到` / `业务` hierarchy 节点才触发导航，避免首页彩色服务卡片被误识别为签到按钮；修复后重试成功：

```text
instance_index: 1
instance_name: 签到实例 1
serial: 127.0.0.1:5557
profile_id: portrait_900x1600_d320
state_history: [locating, locating, locating, ready, ready]
dry_run_ready: true
clicked: false
error: null
```

修复后日志目录：

```text
rust/target/release/logs/2026-09-30_225531/
```

两个实例的 dry-run 都确认了 `locating -> ready` 和连续两帧 ready，且没有点击。

### 实例 0 live

在 dry-run 成功后执行：

```powershell
zhfd-checkin.exe run --live --confirm --instance-index 0
```

结果：

```text
instance_index: 0
instance_name: 签到实例 0
serial: 127.0.0.1:5555
profile_id: portrait_900x1600_d320
state_history: [ready, ready, success]
clicked: true
success: true
error: null
```

点击坐标：

```text
(448, 1076)
```

日志目录：

```text
rust/target/release/logs/2026-09-30_230450/
```

### 实例 1 live

随后执行：

```powershell
zhfd-checkin.exe run --live --confirm --instance-index 1
```

结果：

```text
instance_index: 1
instance_name: 签到实例 1
serial: 127.0.0.1:5557
profile_id: portrait_900x1600_d320
state_history: [ready, ready, success]
clicked: true
success: true
error: null
```

点击坐标同样为：

```text
(448, 1076)
```

日志目录：

```text
rust/target/release/logs/2026-09-30_231230/
```

两个实例均生成独立的 `result.json`、`events.jsonl` 和 3 张状态截图；serial、instance index/name、Profile 和日志目录均未串用。此次完成的是两个实例当前 `portrait_900x1600_d320` Profile 的窗口内 dry-run 与 live 成功闭环，不代表其他分辨率 Profile 已验证。

## 2026-10-01 Review Hardening

根据全量代码审查，本轮已完成以下运行安全和多实例可靠性修复：

- 删除默认配置中的 `instances = []`，空实例列表现在只由 serde 默认值提供；序列化空列表时跳过该字段，避免用户添加 `[[emulator.instances]]` 时产生非法 TOML；
- 同时指定 `--instance-index` 和 `--serial` 时强制校验二者对应同一已发现实例；未知 serial 不再静默回退；
- `--all-instances` 只遍历配置中 `enabled=true` 的实例；不能与显式 index/serial 混用；
- 点击后进入 success-only 等待阶段，即使 UI 延迟帧仍被识别为 ready，也不会重复点击；
- hierarchy 解析遇到畸形属性时继续扫描后续节点，而不是提前返回；
- ADB 截图返回空 stdout 时报告明确的设备 offline/重启错误；
- 普通运行日志和同步日志使用毫秒、PID 和冲突递增号创建唯一目录；
- GUI 默认选择第一个启用实例；应用版本查询任务启动失败时允许后续重试；
- build script 额外监听 Git ref 元数据，减少 About 页面 commit 信息滞后。

本轮离线验证：

```text
cargo fmt --check        PASS
cargo clippy -D warnings PASS
cargo test               PASS（31 tests）
cargo build --release    PASS
portable_smoke           PASS
```

APK manifest 包名预检、CJK 字体内置和 Windows 计划任务路径集成测试仍列为下一批工作；当前 APK 流程仍依赖安装后目标包校验，不能把它误认为已完成安装前包身份验证。

## 2026-10-01 APK、字体、任务命令与点击状态机整改

本轮继续完成审查清单中的离线项：

- 新增 `rust/src/apk.rs`，不依赖 `aapt.exe`，直接读取 APK ZIP 中的 binary `AndroidManifest.xml`；
- 安装前校验 APK package name，GUI 确认窗口展示包名、versionName 和 versionCode；错误包名不会执行安装；
- GUI 使用随 EXE 编译的 `rust/assets/NotoSansSC-VF.ttf` 配置 egui CJK 字体，避免 Windows 便携环境依赖系统中文字体；
- 点击后状态机进入 success-only 等待阶段，延迟 ready 帧不会触发第二次点击；
- hierarchy 解析和空截图错误均有明确处理；
- 日志目录唯一性、双实例配置 round-trip 和 APK 基础解析测试已加入。

本轮验证：

```text
cargo fmt --check        PASS
cargo clippy -D warnings PASS
cargo test               PASS（33 tests）
cargo build --release    PASS
portable_smoke           PASS
```

APK manifest 解析支持标准 APK 的 stored/deflate ZIP 条目和 Android binary XML 的 package、versionName、versionCode；不支持 `.xapk`、`.apks` 或分包安装格式。

## 2026-10-01 Review follow-up completion

本轮完成了剩余离线整改：

- APK 安装前解析标准 APK 的 binary AndroidManifest，校验 package name，并在 GUI 确认窗口显示包名、versionName、versionCode；
- 使用随 EXE 嵌入的 Noto Sans SC 字体配置 egui，修复 Windows GUI 中文方框问题；字体许可证文件随资源提交；
- 抽出计划任务命令行构造函数，并增加带空格 EXE 路径的 Windows 命令字符串测试；
- 将点击流程明确分成点击前/等待 success 阶段，点击后 ready 延迟帧不会再次 tap；
- 新增 APK 解析、任务命令、畸形 hierarchy 和日志目录唯一性测试。

最终离线验证：

```text
cargo fmt --check        PASS
cargo clippy -D warnings PASS
cargo test               PASS（34 tests）
cargo build --release    PASS
portable_smoke           PASS
```

当前仍不支持 `.xapk`、`.apks` 和分包 APK；CJK 字体资源为 `rust/assets/NotoSansSC-VF.ttf`，许可证为 `rust/assets/OFL-NotoSansSC.txt`。

## 2026-10-03 GUI 与 CLI 业务入口统一

CLI 与 GUI 现在共享同一套核心业务函数：

- GUI 的 dry-run/live 运行调用 `run`；
- GUI 的 Profile 检测调用 `runtime_with_target` 和 Profile 匹配逻辑；
- GUI 的 Profile 同步调用 `sync_profile`；
- GUI 的 APK 安装调用 `install_apk`，并在确认前调用 `inspect_apk_metadata`；
- GUI 的实例页调用 `list_instance_summaries`；
- GUI 的 hierarchy 导出调用 `dump_ui_hierarchy`；
- GUI 的截图分析调用 `analyze_image_file`；
- GUI 的计划任务调用 `task_command`。

GUI 不再复制 CLI 的设备、Profile、视觉或安装业务判断；GUI 只负责交互、确认窗口和后台任务调度。

Windows 启动方式已拆分为两个二进制：

```text
zhfd-checkin.exe       CLI dispatcher
zhfd-checkin-gui.exe   windows_subsystem="windows" 的独立 GUI
```

CLI 无参数启动时会以 detached process 启动同目录的 `zhfd-checkin-gui.exe`，不会创建终端窗口。直接双击 `zhfd-checkin-gui.exe` 也不会显示控制台窗口。portable smoke 已要求两个 EXE 同时存在。

本轮离线验证：

```text
cargo fmt --check        PASS
cargo clippy -D warnings PASS
cargo test               PASS（34 tests）
cargo build --release --bins PASS
portable_smoke           PASS
```

## 2026-10-03 GUI/CLI parity follow-up

GUI 现已补齐 CLI 中此前没有对应入口的用户功能：

- 首页提供 live 运行确认窗口，确认后调用与 CLI 相同的 `run` 函数和安全门禁；
- 实例页展示实例列表、运行状态、serial、尺寸/DPI 和 enabled 状态；
- 诊断页可以导出当前实例 hierarchy，并支持选择图片调用同一视觉分析函数；
- Profile、APK、计划任务、报告和日志仍调用 CLI 共用函数，不在 GUI 内复制业务判断；
- CLI 默认入口不再直接创建 GUI，而是以 detached process 启动同目录 `zhfd-checkin-gui.exe`；
- `zhfd-checkin-gui.exe` 使用 Windows GUI subsystem，不创建控制台窗口；
- CI、README 和 portable smoke 均改为构建/检查两个 release binary。

本轮验证：

```text
cargo fmt --check        PASS
cargo clippy -D warnings PASS
cargo test               PASS（34 tests）
cargo build --release --bins PASS
portable_smoke           PASS
```
## 2026-10-07 发布整理与 dry-run 计划任务状态

本轮按 deadline 优先完成发布整理，保留 dry-run 计划任务，不创建 live 自动任务。

当前计划任务：

```text
ZHFD-AutoCheckin-DryRun
执行命令：zhfd-checkin.exe run --dry-run --all-instances
计划时间：每天 21:00（Asia/Shanghai）
```

2026-10-07 21:00 计划任务已实际执行，三个实例日志均独立生成，且 `clicked=false`：

- 实例 0 识别到旧日期的 success 页面；没有点击，但不能把旧日期 success 当作当天签到证明；
- 实例 1、实例 2 实际截图已进入“晚点名签到”页并处于“定位中”，但 hierarchy 路由信息不足，记录为 `navigation_not_confirmed`；
- 任务基础设施、CLI 目标路径、实例选择和 fail-closed 行为正常；
- 当前不创建 live 计划任务。

该结果作为当前发布版本的已知运行限制记录，不阻塞 deadline 优先的便携版发布；后续继续通过 dry-run 计划任务窗口观察并改进页面路由和当天日期校验。

本轮发布整理包括：

- `scripts/package_release.ps1` Windows 便携包脚本；
- GitHub Actions release commit 一致性检查；
- tag 触发的 Windows Release workflow；
- `.gitignore` 对本地 `dist/`、`release/` 暂存目录的忽略；
- CLI/GUI 双 EXE 和 dry-run 计划任务使用说明。

发布包不包含 `config.toml`、账号、Cookie、Token、日志、截图或 ADB 状态。
