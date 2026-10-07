# 项目上下文与进度交接记录

> 更新时间：2026-10-07（Asia/Shanghai）
> 项目：`check-in-zhfd`
> 工作区：`D:\Documents\Projects\check-in-zhfd`

## 1. 当前结论

Rust CLI、Windows GUI、多实例、ADB 重连、Profile 检测、dry-run/live 安全门禁和计划任务入口已经形成可运行闭环。

当前三个 LDPlayer 实例均使用 `900x1600@320`，对应 Profile：

```text
portrait_900x1600_d320
```

2026-10-06 23:52–23:56 的窗口内验证结果：

- 实例 0：实际完成一次 live 点击并识别 success；
- 实例 1：已处于 success，未重复点击；
- 实例 2：首次出现 `unknown_state`，重试后识别 success，未重复点击；
- 三个实例均未出现错误 Profile、ADB 串用或跨实例日志问题。

当前已创建一个 **dry-run 计划任务**，尚未创建 live 计划任务。

## 2. Git 与工作区状态

最近提交：

```text
401f07d fix: gate runs on check-in route and repair GUI all-instance runs
0f17ea0 fix: route GUI scheduled tasks through CLI
13fe809 fix: retry LDPlayer ADB registration before run
fef4fe5 fix: align sign-in window and scroll home navigation
e77dc4b feat: unify GUI actions with CLI and add standalone GUI binary
77ac2cf refactor: split GUI launcher from CLI binary
c1fee9b feat: harden CLI and add multi-instance support
```

当前工作区：

```text
?? docs/RUST_PLAN.md
```

`docs/RUST_PLAN.md` 是用户提供的方案文档，目前没有纳入功能提交。

## 3. 当前架构

### CLI 与 GUI

```text
zhfd-checkin.exe       CLI dispatcher
zhfd-checkin-gui.exe   Windows GUI subsystem 独立 GUI
```

CLI 无参数时以 detached process 启动同目录 GUI；GUI 不创建控制台窗口。

GUI 运行、Profile 检测、Profile 同步、APK 安装、hierarchy 导出、视觉分析、实例列表和计划任务均复用 CLI 核心函数，不在 GUI 内复制核心业务判断。

### 核心安全策略

- 默认模式为 dry-run；
- live 必须显式使用 `--live --confirm` 或 GUI 二次确认；
- live 必须通过 calibrated Profile 门禁；
- 必须处于配置签到窗口；
- 必须确认前台包名正确；
- 必须连续获得 ready 状态；
- 点击前执行二次截图确认；
- 点击后只等待 success，不再重复 tap；
- 任何未知状态、ADB 失败、Profile 不匹配或窗口外状态均 fail-closed。

## 4. 当前配置

正式运行配置：`rust/target/release/config.toml`

```toml
[app]
package_name = "cn.edu.fzu.fdxypa"
activity_name = "cn.edu.fzu.fdxy_app.MainActivity"

[window]
start = "21:00"
end = "23:59"
timezone = "Asia/Shanghai"

[runtime]
mode = "dry-run"
locating_poll_seconds = 10
location_timeout_seconds = 150
success_timeout_seconds = 60
startup_timeout_seconds = 30
stable_frames = 2
unknown_retries = 3
```

实例配置：

| index | 名称 | 默认 serial | 实际 Profile |
|---:|---|---|---|
| 0 | 签到实例 0 | `127.0.0.1:5555` | `portrait_900x1600_d320` |
| 1 | 签到实例 1 | `127.0.0.1:5557` | `portrait_900x1600_d320` |
| 2 | 签到实例 2 | `127.0.0.1:5559` | `portrait_900x1600_d320` |

签到窗口已经从旧的 `21:30–23:59` 统一为当前 App 实际使用的：

```text
21:00–23:59（Asia/Shanghai）
```

历史文档中的 `21:30` 保留为历史验证记录，不代表当前默认值。

## 5. Profile 状态

当前内置 9 个 Profile：

| Profile | calibrated | 说明 |
|---|---:|---|
| `portrait_900x1600_d320` | true | 当前三个 LDPlayer 实例的实际 Profile，已完成窗口内验证 |
| `portrait_900x1600_d240` | true | 已完成历史真实窗口 dry-run/live 验证 |
| `landscape_1600x900_d240` | false | 只完成几何/窗口外能力验证 |
| `landscape_1280x720_d240` | false | 只完成几何/窗口外能力验证 |
| `portrait_720x1280_d240` | false | 只完成几何/窗口外能力验证 |
| `landscape_1920x1080_d280` | false | 只完成几何/窗口外能力验证 |
| `portrait_1080x1920_d280` | false | 只完成几何/窗口外能力验证 |
| `landscape_960x540_d160` | false | 只完成几何/窗口外能力验证 |
| `portrait_540x960_d160` | false | 只完成几何/窗口外能力验证 |

未标定 Profile 仍然禁止 live。

注意：`docs/RUST_MIGRATION.md` 的早期章节仍有“仅 d240 calibrated”等历史表述，需要后续补充当前 d320 状态，避免读者将历史状态误认为当前状态。

## 6. 关键功能进度

### 已完成

- Rust CLI 基础迁移；
- egui GUI；
- GUI/CLI 业务入口统一；
- Windows GUI subsystem 独立 EXE；
- 内置 Noto Sans SC 中文字体；
- 9 个 Profile 定义和严格分辨率/DPI 匹配；
- Profile 同步前后检查、备份和失败恢复；
- ADB 自动 connect/retry；
- 多实例 index/name/serial 隔离；
- 首页下滑查找“晚点名签到”；
- landscape “业务”入口和服务目录滚动；
- 四态视觉识别：gray、locating、ready、success；
- 点击前 ready 二次确认；
- 点击后 success-only 等待；
- 结构化运行日志和 `result.json`；
- APK manifest 包名预检；
- GUI APK 确认窗口和安装结果展示；
- GUI 当前 App 版本展示；
- About 页面 Git commit/Profile/依赖信息；
- portable smoke 检查两个 EXE、`logs/`、`captures/`、`reports/`；
- 计划任务 CLI/GUI 入口统一指向 CLI EXE。

### 当前未完成或仍需增强

- 登录页自动识别和 GUI 提示尚未实现；
- 当前流程遇到登录页时会继续尝试首页下滑，缺少 `login_required` 明确事件；
- 实例 2 在 2026-10-06 23:54 首次出现 `unknown_state`，重试后成功，仍需分析并提高导航/状态稳定性；
- 计划任务尚未完成“手动触发后检查三实例结果”的闭环；
- 尚未创建 live 计划任务；
- UiAutomator Rust crate 尚未引入，当前继续使用 `adb shell uiautomator dump`；
- 不支持 `.xapk`、`.apks` 和 split APK；
- 尚未完成干净 Windows 环境验收；
- `docs/RUST_MIGRATION.md` 需要追加 2026-10-06 三实例 live 与计划任务进度。

## 7. ADB 修复验证

提交：

```text
13fe809 fix: retry LDPlayer ADB registration before run
```

`runtime_with_target()` 现在会在 LDPlayer 显示 running 但 ADB 尚未注册时：

1. 执行 `adb connect`；
2. 检查设备是否为 `device`；
3. 每 500 ms 重试；
4. 使用 `startup_timeout_seconds` 作为上限；
5. 超时后返回明确错误。

实例 1 的断开/恢复验证已通过：

```text
adb disconnect 127.0.0.1:5557
zhfd-checkin.exe profile detect --instance-index 1
```

最终成功恢复：

```text
serial=127.0.0.1:5557
size=900x1600
dpi=320
profile=portrait_900x1600_d320
```

## 8. 2026-10-06 三实例验证记录

### 8.1 窗口外安全 dry-run

执行时间约为 2026-10-06 15:00，尚未进入窗口。

三个实例均返回：

```json
{
  "clicked": false,
  "success": false,
  "dry_run_ready": false,
  "error": "outside_window"
}
```

结果确认：

- index、name、serial 正确；
- Profile 正确；
- 三个实例均未点击；
- 日志目录独立；
- 窗口外门禁生效。

对应日志：

```text
rust/target/release/logs/2026-10-06_150055_516_36308/  # 实例 0
rust/target/release/logs/2026-10-06_150055_504_39296/  # 实例 1
rust/target/release/logs/2026-10-06_150055_806_7960/  # 实例 2
```

### 8.2 实例 1 登录页确认

2026-10-06 下午，实例 1 前台包和 Activity 正常，但页面停留在登录界面：

```text
cn.edu.fzu.fdxypa/cn.edu.fzu.fdxy_app.MainActivity
```

hierarchy 显示：

```text
一键登录
其他账户登录
点击注册
```

截图和 hierarchy：

```text
rust/target/release/captures/instance1-current.png
rust/target/release/captures/instance1-current.xml
```

用户随后手动完成“一键登录”。当前代码没有自动点击登录，也没有自动输入账号/验证码。

### 8.3 窗口内 GUI live

用户在 2026-10-06 23:52–23:56 窗口内依次运行三个实例的 GUI live。

#### 实例 0

日志：

```text
rust/target/release/logs/2026-10-06_235254_606_37772/
```

结果：

```text
state_history = ready -> ready -> success
clicked = true
success = true
serial = 127.0.0.1:5555
```

结论：实际点击成功。

#### 实例 1

日志：

```text
rust/target/release/logs/2026-10-06_235319_862_37772/
```

结果：

```text
state_history = success
clicked = false
success = true
serial = 127.0.0.1:5557
```

结论：识别到已经签到成功，没有重复点击。

#### 实例 2

首次运行日志：

```text
rust/target/release/logs/2026-10-06_235438_774_37772/
```

结果：

```text
state_history = unknown -> unknown -> unknown
clicked = false
success = false
error = unknown_state
```

重试日志：

```text
rust/target/release/logs/2026-10-06_235609_075_37772/
```

结果：

```text
state_history = success
clicked = false
success = true
serial = 127.0.0.1:5559
```

结论：重试后识别到已经签到成功，没有点击。

## 9. 计划任务状态

已创建任务：

```text
ZHFD-AutoCheckin-DryRun
```

当前查询结果：

```text
TaskName:      \ZHFD-AutoCheckin-DryRun
Next Run Time: 2026/10/07 21:00:00
Status:        Ready
Run As User:   Bar
Schedule Type: Daily
```

实际执行命令：

```text
"D:\Documents\Projects\check-in-zhfd\rust\target\release\zhfd-checkin.exe" run --dry-run --all-instances
```

已确认没有指向：

```text
zhfd-checkin-gui.exe
```

当前尚未创建：

```text
ZHFD-AutoCheckin-Live
```

## 10. 构建与测试状态

最近一次源码测试：

```text
cargo fmt --check       PASS
cargo clippy -D warnings PASS
cargo test              PASS（35 tests）
cargo build --release --bins PASS
portable_smoke          PASS
```

portable smoke 已确认：

```text
PORTABLE_SMOKE_PASS
profile_count=8+
vision_state=ready
runtime_dirs=logs,captures,reports
```

正式 EXE：

```text
rust/target/release/zhfd-checkin.exe
rust/target/release/zhfd-checkin-gui.exe
```

注意：当前 release EXE 的文件时间为 2026-10-07 00:21，而提交 `0f17ea0` 的提交时间为 00:28。源码中的计划任务修复已经通过测试并提交，但正式 EXE 的 About 内嵌 Git commit 可能仍显示前一个提交 `13fe809ca79d`。下一步应在当前 HEAD 重新构建，以同步二进制版本信息。

## 11. 下一步建议

### 优先级 P0：重新构建当前 HEAD

```powershell
cargo build --release --manifest-path .\rust\Cargo.toml --bins
.\scripts\portable_smoke.ps1
.\rust\target\release\zhfd-checkin.exe about
```

确认 About 显示：

```text
Git commit: 0f17ea0...
```

### 优先级 P1：手动触发 dry-run 计划任务

```powershell
schtasks /Run /TN "ZHFD-AutoCheckin-DryRun"
```

等待任务完成后，检查 `rust/target/release/logs/` 中三个最新 `result.json`：

- `instance_index` 分别为 0/1/2；
- serial 分别为 5555/5557/5559；
- 每个实例目录不同；
- `clicked=false`；
- 不启动 GUI 代替 CLI。

如果在签到窗口内触发，期待：

```text
state_history 包含 locating -> ready 或直接 success
error = null
```

### 优先级 P1：处理登录页

建议增加登录页识别：

```text
检测 content-desc="一键登录" 或 "其他账户登录"
→ 写入 login_required 事件
→ GUI 显示“请先手动登录”
→ 停止首页下滑和签到轮询
```

不建议自动点击“一键登录”，除非明确确认不会触发账号选择、验证码或敏感操作。

### 优先级 P2：分析实例 2 的 unknown_state

保留并分析实例 2 首次失败时的截图，区分：

- 首页尚未完成滚动；
- WebView 尚未加载；
- 已进入签到页但按钮不在 fallback 区域；
- 已签到成功但截图处于过渡帧；
- 导航路径与实例 0/1 不同。

可增加页面稳定等待、成功态优先检测和导航后 hierarchy 复核。

### 优先级 P2：补正文档

更新 `docs/RUST_MIGRATION.md`：

- 当前 d320 Profile 已 calibrated；
- 三实例窗口内 live 结果；
- 实例 1 登录页现象；
- 实例 2 首次 unknown 后重试成功；
- dry-run 计划任务状态；
- 当前 release binary 需要重新构建的说明。

### 优先级 P3：最终自动化策略

只有在以下条件满足后再创建 live 计划任务：

- dry-run 计划任务已手动触发并核验；
- 登录状态具备明确处理策略；
- 实例 2 unknown 问题有可解释性或稳定重试策略；
- 任务运行用户、ADB 连接和 LDPlayer 自动启动已验证；
- 明确接受 live 任务可能真实点击三个实例。

## 12. 安全约束

- 不要在窗口外伪造 ready 或修改系统时间；
- 不要为了验证 live 绕过 calibrated、前台包名、连续 ready 或二次确认门禁；
- 当前只允许 dry-run 计划任务；
- 任何新实例或新 Profile 先 dry-run，再决定 live；
- 不要提交 `rust/target/` 生成物、日志、截图或账号信息；
- 不要把 `docs/RUST_PLAN.md` 自动加入功能提交，除非用户明确要求。
## 13. Deadline 优先发布决策（2026-10-07）

用户决定不创建 live 自动任务，先保留并继续测试 `ZHFD-AutoCheckin-DryRun`。由于上线 deadline 优先，当前发布包接受以下已知限制：

- 计划任务基础设施已在 2026-10-07 21:00 实际运行；
- 实例 0/1/2 均生成独立日志和 `result.json`，均 `clicked=false`；
- 实例 0 的 success 页面可能是前一日旧状态；
- 实例 1/2 的截图已在签到页，但 hierarchy 路由可能记录 `navigation_not_confirmed`；
- 不创建 live 任务，不把这次结果宣传为三实例当天签到全部成功。

正式整理发布内容：

- 双 EXE release 构建；
- `scripts/package_release.ps1` 便携包脚本；
- GitHub Actions release 构建和 commit 一致性检查；
- `.gitignore` 忽略本地发布暂存目录；
- 发布说明明确默认 dry-run 和已知限制。

后续仍以 dry-run 计划任务为主要回归入口，优先观察窗口内的登录状态、页面路由、当天日期和三实例日志隔离。
