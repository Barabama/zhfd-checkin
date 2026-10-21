# 签到页面状态与安全策略

更新时间：2026-10-21（Asia/Shanghai）

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

外勤模式与普通签到分离；层级同时确认晚点名页、未签到状态及“外勤签到”入口/按钮证据时生成 `field_work_ready` dry-run 观察结果。**外勤 live 默认关闭**；只有用户在当前 data root 的 `config.toml` 中明确设置 `runtime.allow_field_work_live = true`，并使用 `--live --confirm`，才允许进入外勤 live 的共享状态机。该 opt-in 不会把外勤当作普通签到，也不会绕过 Profile、时间窗、前台、连续 Ready、点击前二次 route/mode 复核或 `guarded_tap` 的其他门禁。

外勤 live 使用外勤页面当前识别出的按钮坐标；按钮必须由本轮截图分类为 Ready，且点击前重新读取 hierarchy/前台包名并确认仍是 `Checkin + FieldWork`。Unsupported、证据冲突或任一门禁不足时停止且不点击。

## 登录页与一键登录

默认行为：识别到 `Login` 后以 `login_required` 停止，要求用户在模拟器中手动登录。程序不输入账号、密码、验证码，不伪造定位或设备身份。

可选的自动登录（ADR-0004）只在**同时**满足以下条件时执行一次「一键登录」：

- `runtime.allow_auto_login = true` 且 `runtime.accept_login_terms = true`（后者表示授权程序代勾选《用户协议》《隐私协议》）；
- `--live --confirm`、目标 App 前台、无外部窗口；
- `route=Login` 且来自目标包节点；
- 「一键登录」节点唯一、可点击；同意复选框可定位，点击后经截图确认已勾选；
- 配置了 `runtime.auto_login_account` 时，页面显示的“上次使用过的账号”必须精确匹配；
- 点击前重新观测仍是同一登录页、同一账号、无遮挡。

限制与失败语义：

- 只支持一键登录，不处理「其他账户登录」，不输入/保存任何凭据；
- 登录页选择与点击坐标来自 hierarchy bounds，不依赖 Profile 几何；但 live 仍需目标 Profile 已标定，未标定预设会在启动前以 `uncalibrated_profile` 停止；
- 一次运行最多一次登录点击，失败不重试；
- dry-run 永不点击，只写 `auto_login_ready`；
- 失败使用稳定的 `auto_login_*` 错误码（`terms_not_accepted`、`consent_unconfirmed`、`account_mismatch`、`blocked`、`timeout`、`not_confirmed` 等）；
- `auto_login_succeeded=true` 只表示已离开登录页，不代表签到成功；`clicked` 仍只表示签到按钮被点击。

## 旧签到页刷新（stale page refresh）

签到成功后程序不主动退出 App、不发送额外的 BACK，也不会把 App 拉回前台或重启（FR-013）。若某次运行启动时已停留在旧的签到页，程序只做一次受控的 BACK 刷新：

- 仅在层级确认签到页且无外部窗口时发送 BACK；
- BACK 后重新读取前台包名与页面层级；若首次读取层级不可确认，按既有上限重读若干次；
- 只有确认回到目标 App 的 Home/Services 页（前台仍是目标包且无遮挡）才继续导航；
- 若返回后层级持续不可读或无法确认，记 `stale_page_refresh_unconfirmed` 并停止；
- 若返回后可读但不是目录页，且也不是目标 App 的签到页，记 `stale_page_refresh_failed` 并停止；
- 若返回后目标 App 已不在前台（例如退到了桌面/Launcher），记 `app_left_foreground`（机器码复用 FR-013 的 `target_app_not_foreground`）并停止，且不把该前台包记录为弹窗遮挡。

**BACK 被吞时的回退（ADR-0002）**：部分 App 构建会忽略签到页上的 BACK（实测连续按键后层级字节完全不变）。当 BACK 后仍满足「目标 App 前台 + 无外部窗口 + 层级确认签到页」时，程序不再判刷新失败，而是继续使用该页并进入正常状态机。所有门禁保持不变：每轮重新观察 route/mode/前台，点击前再次复核，`success=true` 仍要求当天日期。该路径在事件中记录 `fallback=continue_on_checkin_page`，结果中表现为 `page_refresh_attempted=true` + `page_refresh_confirmed=false`。

除上述回退外，任何无法确认的情况都在任何 tap 之前停止（`clicked=false`），不尝试自动拉起或重新启动 App；恢复动作由用户手动完成。

## 成功判定

当天成功必须有：绿色成功视觉 + 明确“签到成功/已签到”层级文本 + 可解析的页面日期，并且日期按配置时区等于当前日期。日期不匹配记 `success_stale`；日期缺失记 `success_date_unknown`，都不将 `success=true`。仅 Ready 的 dry-run 是 `dry_run_ready`，不是签到成功。

## 结果/证据语义

`result.json` 保存 route、mode、state、日期、是否点击、错误阶段、恢复建议和 artifact paths；`events.jsonl` 保存结构化事件；截图是证据附件。为避免个人/定位信息扩散，运行日志不将原始 hierarchy 当常规 artifact 写入。分享诊断报告前仍应人工检查本地路径和个人信息。

独立审计的点击、日期、未知状态反例和未验证项见 [当前审计报告](./audits/PHASE4_INDEPENDENT_AUDIT_REPORT_2026-10-11.md)。
