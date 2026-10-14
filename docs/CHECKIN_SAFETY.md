# 签到页面状态与安全策略

更新时间：2026-10-11（Asia/Shanghai）

## 三个独立维度

- `PageRoute`: Login / Home / Services / Checkin / Unknown / RouteCandidateUnconfirmed / RouteEvidenceConflict / StalePageRefreshFailed / StalePageRefreshUnconfirmed / AppLeftForeground
- `CheckinMode`: Ordinary / FieldWork / Unsupported
- `CheckinState`: Gray / Locating / Ready / SuccessToday / SuccessStale / SuccessDateUnknown / FieldWorkReady / Unknown

不能由按钮颜色单独推断页面路线或业务类型。蓝/绿视觉状态不是自动授权证据。

## 普通签到 live 门禁

所有条件必须同时成立：

```text
mode=Live AND explicit confirmation
AND route=Checkin AND checkin_mode=Ordinary
AND profile calibrated AND target App foreground
AND inside configured window AND stable Ready frames
AND second Ready confirmation AND this run has not tapped
```

任何未知、失败或页面切换都应停止。点击后只等待结果，不得再次 tap。dry-run 永远不能调用 tap。

## 外勤/未知模式

外勤模式与普通签到分离；当前实现只在层级同时确认晚点名页、未签到状态及“外勤签到”入口/按钮证据时生成 `field_work_ready` dry-run 观察结果。外勤 Live 明确拒绝。Unsupported 或 hierarchy 证据不足时停止并给恢复建议。

本工具不自动输入账号、密码、验证码，不自动点击一键登录，不伪造定位或设备身份。遇登录页，要求用户在模拟器手动登录。

## 旧签到页刷新（stale page refresh）

签到成功后程序不主动退出 App、不发送额外的 BACK，也不会把 App 拉回前台或重启（FR-013）。若某次运行启动时已停留在旧的签到页，程序只做一次受控的 BACK 刷新：

- 仅在层级确认签到页且无外部窗口时发送 BACK；
- BACK 后重新读取前台包名与页面层级；
- 只有确认回到目标 App 的 Home/Services 页（前台仍是目标包且无遮挡）才继续导航；
- 若返回后层级不可读或无法确认，记 `stale_page_refresh_unconfirmed` 并停止；
- 若返回后可读但不是目录页，记 `stale_page_refresh_failed` 并停止；
- 若返回后目标 App 已不在前台（例如退到了桌面/Launcher），记 `app_left_foreground`（机器码复用 FR-013 的 `target_app_not_foreground`）并停止，且不把该前台包记录为弹窗遮挡。

以上任何一种情况都在任何 tap 之前停止（`clicked=false`），不尝试自动拉起或重新启动 App；恢复动作由用户手动完成。

## 成功判定

当天成功必须有：绿色成功视觉 + 明确“签到成功/已签到”层级文本 + 可解析的页面日期，并且日期按配置时区等于当前日期。日期不匹配记 `success_stale`；日期缺失记 `success_date_unknown`，都不将 `success=true`。仅 Ready 的 dry-run 是 `dry_run_ready`，不是签到成功。

## 结果/证据语义

`result.json` 保存 route、mode、state、日期、是否点击、错误阶段、恢复建议和 artifact paths；`events.jsonl` 保存结构化事件；截图是证据附件。为避免个人/定位信息扩散，运行日志不将原始 hierarchy 当常规 artifact 写入。分享诊断报告前仍应人工检查本地路径和个人信息。

独立审计的点击、日期、未知状态反例和未验证项见 [当前审计报告](./audits/PHASE4_INDEPENDENT_AUDIT_REPORT_2026-10-11.md)。
