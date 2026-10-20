# 快速开始：从首次设置到 dry-run

这份指南面向第一次安装的用户。目标是：配置好本机 LDPlayer/ADB，在一个实例上完成一次 dry-run，并知道如何检查结果。第一次请不要直接运行全部实例，也不要启用 live。

## 1. 开始前准备

- Windows 电脑上已安装并能启动 LDPlayer。
- 准备好本机的 `ldconsole.exe` 和 `adb.exe` 路径；通常可在 LDPlayer 安装目录找到。
- 已有可写的程序目录。便携版会在数据根目录保存 `config.toml`、运行日志、截图、报告和备份。
- 在模拟器中由用户手动安装/打开智汇福大 App，并完成登录。程序不会代为输入账号、密码、验证码或同意协议。

下载包的版本和发布状态请以实际发布页面为准；源码工作区构建不等同正式发布包。更多部署说明见 [OPERATIONS.md](./OPERATIONS.md)。

## 2. 启动并完成首次设置

1. 启动 LDPlayer，并保持目标模拟器实例可用。
2. 启动 `zhfd-checkin-gui.exe`。
3. 首次启动如果进入“首次设置 / 配置修复”，确认 `ldconsole.exe` 和 `adb.exe` 路径。
   - 浏览选择 `ldconsole.exe` 后，程序可能建议同目录的 `adb.exe`；这是候选路径，保存前请确认。
   - 确认实例名称、index、serial 和 enabled 状态与模拟器相符。
4. 执行依赖验证；验证通过后保存配置。
5. 如果配置损坏，GUI 会显示 Repair 提示。原配置不会被静默覆盖；先阅读错误和备份信息，再选择重新载入、加载其他配置或显式恢复默认配置。

如果不知道工具位置，可先在“环境诊断”查看自动发现的候选，再回设置页确认路径。路径、实例和 data root 的详细行为见 [OPERATIONS.md](./OPERATIONS.md)。

## 3. 先运行环境诊断

在 GUI 打开“环境诊断”，选择一个实例并运行检测。确认：

- 配置中的模拟器控制台和 ADB 工具可用；
- 选定的实例/serial 是目标模拟器；
- 模拟器已启动，并在运行前由用户手动确认目标 App 位于前台。

诊断页确认的是依赖、实例、ADB 设备信息和 Profile；它不会代替用户确认目标 App 前台，也不会执行签到点击。若诊断提示实例离线、路径错误或 Profile 不支持，请先处理该问题，再进入下一步。

## 4. 执行第一次单实例 dry-run

1. 回到“运行控制”。
2. 选择一个要测试的实例；第一次保持“dry-run 依次运行全部已发现实例”未勾选。
3. 确认模拟器界面已由你手动打开到合适状态，再点击“运行 dry-run”。
4. 等待任务完成，不要同时手动操作同一个模拟器。

**dry-run 不执行签到点击**，但运行可能会启动目标 App、读取页面、在已确认的页面间导航或等待状态。因此它不是纯离线预览；请在你有权操作的设备上运行，并留意模拟器界面。

## 5. 确认结果

打开“运行日志与结果”，检查最新一次运行：

- `status` 表示运行结果：`success`、`failed` 或 `blocked`；dry-run 达到可观察状态时顶层 `status` 也可能是 `success`，不要把它单独理解成真实签到成功；
- `page_route` / `route` 表示识别到的页面；
- `checkin_mode` 表示签到业务类型；
- `checkin_state` 表示按钮/签到状态；
- `error_code` 和恢复建议说明失败原因与下一步；
- 事件时间线、文本结果和截图文件可以帮助复核运行过程；GUI 日志页可预览事件和 JSON 文本，截图请使用“打开目录”在文件夹中查看。

特别注意：`dry_run_ready=true` 只表示观察到可签到状态，**不是已经签到成功**。只有 `success=true` 才表示程序按当前成功规则确认了签到结果。dry-run 本身不会造成签到点击，因此通常不会因为 dry-run 而得到真实签到成功。

### 遇到常见结果时

| 结果或现象 | 建议处理 |
|---|---|
| `device_setup_failed` / ADB 设备不可用 | 启动模拟器，确认路径、实例 index 和 serial，再回“环境诊断”复查。 |
| `login_required` | 在模拟器中手动登录，再重新运行。默认不会自动登录；只有显式开启 `runtime.allow_auto_login` 与 `runtime.accept_login_terms` 后，live 运行才会尝试一次一键登录。 |
| `overlay_present` | 根据日志中的包名检查遮挡窗口；在模拟器中手动关闭弹窗/广告后，再重新运行。程序不会自动关闭或点击弹窗。 |
| `route_candidate_unconfirmed` | 不要让程序猜页面；查看最新截图和层级诊断，手动确认页面后再决定是否重试。 |
| `route_evidence_conflict` | 截图和页面层级证据不一致；停止自动操作，检查模拟器和日志后再处理。 |
| `stale_page_refresh_failed` | 启动时停留在旧签到页，返回后没回到服务页；在模拟器中手动返回首页后重新运行。程序未点击。 |
| `stale_page_refresh_unconfirmed` | 启动时停留在旧签到页，返回后页面无法确认；在模拟器中手动返回首页后重新运行。程序未点击。 |
| `app_left_foreground` / `target_app_not_foreground` | 返回后目标 App 已不在前台（可能退到了桌面）；在模拟器中手动把 App 的普通签到页切回前台后重新运行。程序不会自动拉起或重启 App。 |
| `field_work_live_unsupported` | 检测到外勤签到。默认不执行 live 点击；如经风险确认确需启用，请在当前 data root 的 `config.toml` 中设置 `runtime.allow_field_work_live = true`，再由用户明确运行 `--live --confirm`。 |
| `location_wait_timeout` | 定位等待达到上限；检查模拟器中的定位状态，之后由用户决定是否重试。定位中不会点击。 |
| `blocked` / `shared_preflight_failed` | 公共依赖检查失败；先修复依赖路径，批次没有启动设备操作。 |
任何页面未知、证据冲突或意外弹窗都应先停止并人工确认。不要为了“试出来”而连续盲目运行。

## 6. 日常使用

- **日常检查**：选择目标实例，先运行 dry-run；从“运行日志与结果”核对 route、mode、state 和截图。
- **外勤 live**：默认关闭。如果用户经风险确认决定启用，在“配置”页勾选外勤 live 授权（或在当前 data root 的 `config.toml` 设置 `runtime.allow_field_work_live = true`），保存后再由用户明确运行 `--live --confirm`；先完成外勤页面 dry-run，确认实例、定位和按钮后再执行。
- **多实例**：先逐个确认实例映射与单实例 dry-run，再按需要选择全部已启用实例。批量结果会同时写入实例级 `result.json` 和批次 `summary.json`。
- **配置修改**：通过“配置文件”或“首次设置 / 配置修复”调整；覆盖配置前会创建备份。迁移数据目录后会记录新的数据根目录（`%LOCALAPPDATA%\ZHFDCheckin\data-root.txt`），重启仍使用新目录；源目录保留不删除。
- **长期计划任务**：优先使用 dry-run 计划任务。不要将 live 配置为无人值守任务。
- **分享诊断资料**：导出报告或截图前检查其中的本机路径、实例信息和可能包含个人信息的画面。

## 7. live 模式须知

本指南不会指导用户把首次运行直接切到 live。live 会尝试真实签到点击，必须由用户明确发起并确认；普通签到仍需满足目标页面、普通业务模式、校准 Profile、前台应用、配置时间窗、连续 ready 状态和点击前再次确认等全部门禁。外勤 live 默认关闭；只有在当前 data root 的 `config.toml` 明确设置 `runtime.allow_field_work_live = true` 后，才会进入外勤独立点击路径，并继续满足同等门禁。未知模式、弹窗遮挡、页面未确认或任何门禁不满足时都不会点击。

live 的完整安全契约见 [CHECKIN_SAFETY.md](./CHECKIN_SAFETY.md)。如不确定某个结果或页面状态，请停留在 dry-run 并先检查日志。

## 8. 日志和数据位置

程序在数据根目录保存配置、日志、截图、报告和备份。便携部署默认使用 EXE 目录；显式 `--config` 时，配置文件的父目录作为 data root。GUI 的运行日志页可查看最近结果；诊断报告适合汇总检查，但分享前仍要自行脱敏。

需要更详细的 CLI 命令、路径发现、多实例和数据迁移说明时，继续阅读 [OPERATIONS.md](./OPERATIONS.md)。
