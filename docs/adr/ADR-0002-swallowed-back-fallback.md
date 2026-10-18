# ADR-0002：BACK 被 App 吞掉时继续使用已确认的签到页

- 状态：已接受
- 日期：2026-10-18
- 影响文档：`docs/CHECKIN_SAFETY.md`「旧签到页刷新」

## 背景

v0.3.0 的旧签到页刷新策略要求：启动时若已在签到页，先发送一次 BACK，只有回到
Home/Services 才继续；否则按 `stale_page_refresh_failed` / `unconfirmed` /
`app_left_foreground` 停止。

2026-10-17 现场验证发现：部分 App 构建会完全吞掉签到页上的 BACK —— 连续发送 3 次
`input keyevent 4`，层级文档字节数完全不变，目标 App 始终保持前台。结果是
`--all-instances` 中停留在成功页的实例必然失败，即使签到本身已经完成。

## 决策

当 BACK 后仍满足以下全部条件时，继续使用当前页面，而不是判为刷新失败：

1. `route == PageRoute::Checkin`（层级仍确认是签到页）；
2. 没有外部窗口（`foreign_package == None`）；
3. 目标 App 仍在前台。

继续后，页面会再次被观察（`navigation_after_action`），并进入与普通流程相同的
状态机；所有门禁不做任何放宽：

- 每轮重新读取 hierarchy、前台包名和截图；
- 点击仍要求 `route=Checkin` + 授权模式 + 校准 Profile + 窗口内 + 连续 Ready；
- `success=true` 仍要求「签到成功/已签到」文本 + 可解析日期 + 日期等于今天。
  旧日期的成功页只会得到 `success_stale`，不会被当作本次成功。

同时，BACK 后的首次观察若是 `route_candidate_unconfirmed`，会按既有的
`should_reobserve` 规则有限次重读，避免 WebView 重绘被误判为刷新失败。

## 可审计性

- `events.jsonl` 记录
  `page_refresh` + `"fallback":"continue_on_checkin_page"`；
- `result.json` 保留 `page_refresh_attempted=true` 与
  `page_refresh_confirmed=false`，消费者可据此识别该路径。

## 影响与回滚

- 该路径只影响「启动时已在签到页且 BACK 无效」的场景；Home/Services 刷新成功时行为不变。
- 不存在自动拉起、重试点击或跨页面点击。
- 回滚方式：删除 `may_continue_on_checkin_page` 分支，恢复为直接返回
  `stale_page_refresh_failed`。
