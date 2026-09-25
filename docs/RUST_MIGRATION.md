# Rust 迁移状态（2026-09-25）

## 当前交付范围

Rust 第一阶段 CLI 已建立，Python 仍是行为参考基线。当前 CLI 使用：

- `image` + 手写 HSV、颜色覆盖率、文字行投影和连通区域搜索；
- ADB 命令实现截图、启动 App、点击、滑动、hierarchy dump、APK 安装；
- LDPlayer `ldconsole.exe` 自动发现、实例读取、启动和 profile 同步入口；
- `egui/eframe` 基础 GUI；
- `config.toml`、`logs/`、`captures/` 均位于 EXE 所在目录。

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

只有已标定 profile 才允许 `--live`；未标定的八个新预设目前可被检测和 dry-run，但不会盲目真实点击。

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

- 八个 profile 的 `locating -> ready` 实态和最终 live 能力验证仍未完成；当前八个均已完成灰态截图、坐标测量和窗口外 dry-run。
- GUI 的更完整异步任务状态、诊断报告导出和同步确认对话框；
- UiAutomator Rust crate 的独立封装验证。目前使用 ADB 的 `uiautomator dump` 作为兼容方案；
- 干净 Windows 环境上的便携 EXE 验收。

## 2026-09-25 `portrait_900x1600_d240` 首轮实机验证

已完成以下操作：

- `profile sync --id portrait_900x1600_d240 --confirm` 首次写入配置后，旧实现因 LDPlayer 停止/启动竞态报告启动超时；随后修复为等待实例明确停止后再启动。
- 修复后的同步命令于 2026-09-25 17:43 再次执行成功，并生成 LDPlayer 配置备份。
- `diagnose` 和 `profile detect` 均识别为 `900x1600@240`、`portrait_900x1600_d240`。
- 实机截图测得 240 DPI 下灰色签到圆约为 `(449,778)`、`232x232`；已更新该 profile 的 dry-run 回退区域，但仍保持 `calibrated=false`。
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

八个 profile 仍全部保持 `calibrated=false`。这表示现在已经完成几何和页面能力验证，但没有把窗口外的灰态结果误当成 ready，也没有绕过应用的时间/定位校验。

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
