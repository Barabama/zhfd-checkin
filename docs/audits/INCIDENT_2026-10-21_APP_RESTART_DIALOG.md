# 事件记录：App「是否重启App」安全弹窗导致 21:00 live 失败

- 日期：2026-10-21
- 受影响版本：`v0.3.2`（远程构建 `d63d3b3a38af`）
- 现场：`10.144.144.9`（`MCMF-429`），`F:\zhfd-checkin-windows-x64`
- 日志：`logs/2026-10-21_210005_437_9688`
- 结论：**App 自身的反劫持弹窗**阻塞了页面识别；不是签到按钮误点，也不是 ADB/实例失败。

## 1. 现象

21:00:05 的计划任务（`run --live --confirm --instance-index 0`）在 navigation 阶段失败：

```text
status        = failed
error_code    = route_candidate_unconfirmed
hierarchy_status = route_markers_missing
page_route    = route_candidate_unconfirmed
clicked       = false
foreground    = cn.edu.fzu.fdxypa
```

三次 hierarchy dump 均为 4359 bytes / 13 nodes，marker 为空；`navigation_result.png` 显示：

- 背景：旧的签到页（显示前一天的“签到成功”）；
- 前景：App 弹窗「是否重启App / 当前 App 可能被界面劫持」，按钮为「取消」「重启App」。

程序没有点击任何按钮（`clicked=false`），行为本身是安全的；问题在于失败原因不可诊断，且计划任务无法自愈。

## 2. 现场复核

- 远程 EXE 已更新为 `0.3.4`（`3437b1f`），21:57 的手工 live 运行成功：
  - `status=success`、`clicked=true`、`success_date=2026-10-21`；
  - 页面 `checkin/ordinary`，状态 `ready → ready → success`。
- App 版本 `1.17.4`，`lastUpdateTime=2026-10-08`，未在 21:00 前后更新。
- `enabled_accessibility_services=null`；诊断时窗口列表没有第三方 overlay。
- 计划任务动作：
  `"F:\zhfd-checkin-windows-x64\zhfd-checkin.exe" run --live --confirm --config "F:\...\config.toml" --instance-index 0`
- 远程 config：`mode=dry-run`、`allow_field_work_live=true`；没有 `allow_auto_login`/`accept_login_terms`（0.3.4 默认 false）。
- 21:57 logcat 可见 `onActivityRestartAttempt` + `START ... LAUNCH_SINGLE_TOP`，说明启动时 activity 被重启；21:00 的弹窗很可能与 activity 重启/反劫持检测有关，但**无法稳定复现**。

## 3. 处理方案（随 v0.3.5）

1. **显式识别弹窗**
   - 新增 `PageRoute::AppRestartDialog`，匹配：
     - `是否重启App`
     - `当前 App 可能被界面劫持`
     - `重启App` + `取消`
   - 优先于签到/登录/首页标记；不再报模糊的 `route_candidate_unconfirmed`。
2. **fail-closed，不自动点击**
   - 返回 `error_code=app_restart_dialog`，`clicked=false`；
   - `recovery_action` 提示手动「取消」或「重启App」，并检查悬浮窗/录屏/辅助功能。
3. **减少触发**
   - 启动时若目标 App 已在前台，跳过 `am start`，记录 `app_launch_skipped`；
   - 避免无意义的 activity restart。
4. **保留证据**
   - 仍保存 `navigation_result.png`；`marker_summary` 现在会包含弹窗文字，日志可直接定位。
5. **不做的**
   - 不自动点击「取消」或「重启App」；该弹窗是 App 的安全提示，自动绕过需要单独的安全决策（可作为后续 opt-in）。

## 4. 残余风险

- 弹窗可能在下次计划任务再次出现；0.3.5 会让失败可诊断，但仍需人工处理。
- 如果现场需要无人值守自愈，可评估 opt-in 的“只点一次取消”策略；需要 ADR 与现场验证，不能默认开启。
- 0.3.2 的日志没有保存原始 hierarchy XML；下次出现时建议用 `zhfd-checkin.exe ui dump --instance-index 0` 采集完整层级（注意含 PII，不要提交）。

## 5. 部署检查

- [ ] 将 0.3.5 EXE 覆盖到 `F:\zhfd-checkin-windows-x64\`
- [ ] `zhfd-checkin.exe about` 确认 `0.3.5` 与干净构建
- [ ] `task query` 确认动作仍指向该 EXE 与 `config.toml`
- [ ] 在下次 21:00 前做一次 `run --dry-run --instance-index 0`
- [ ] 若再出现 `app_restart_dialog`，人工点击「取消/重启App」后重跑，并采集截图/层级

> 本记录不包含真实账号、姓名、班级或定位信息；远程日志只保存在本机临时目录，未提交。
