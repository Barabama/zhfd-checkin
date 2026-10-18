# ADR-0001：外勤签到 live 显式 opt-in

- 状态：已接受（事后补记，2026-10-18）
- 变更日期：2026-10-14（`v0.3.1` / `a955d0a`）
- 影响决策：`docs/V0.3.0_DECISIONS.md` D-08；`docs/V0.3.0_FIRST_RUN_AND_RECOVERY_REQUIREMENTS.md` §10

## 背景

v0.3.0 的冻结决策把外勤签到（`CheckinMode::FieldWork`）限定为 dry-run 观察，live 明确拒绝。
2026-10-14 现场运行表明：用户账号在校外时，App 只提供「外勤签到」按钮，普通签到按钮不存在；
因此原策略下 live 在该设备上永远无法完成签到。

## 决策

1. 新增 `runtime.allow_field_work_live`（默认 `false`）。旧配置没有该键时同样解析为 `false`。
2. dry-run 行为不变：只观察、保存证据、输出 `field_work_ready`，永不点击。
3. live 未启用该开关时继续返回 `field_work_live_unsupported` 并停止。
4. 启用后，外勤仍走**独立模式**，不降级为普通签到：
   - `authorize_page_action` 只有在 `route=Checkin` 且
     （`Ordinary` 或 `FieldWork && allow_field_work`）时授权；
   - `Unsupported` 在任何配置下都拒绝；
   - 继续经过 `--live --confirm`、校准 Profile、时间窗口、目标 App 前台、
     连续 Ready 帧、点击前 route/mode/前台二次复核和 `guarded_tap`。
5. GUI 配置页提供高风险开关；文档明确默认关闭。

## 影响

- 默认行为不改变，旧配置无需迁移。
- 操作员必须显式编辑配置或勾选 GUI 开关，才能让外勤进入 live。
- 外勤页面布局与普通签到不同；未检测到彩色按钮时会退回 profile 几何，
  该限制记录在审计报告 P3 中，后续可改为基于 hierarchy bounds 的点击目标。

## 兼容性与回滚

- `RunResult` 字段不变；`checkin_mode=field_work` 仍与 `ordinary` 分离。
- 回滚方式：把 `runtime.allow_field_work_live` 设为 `false`，外勤立即恢复为观察模式。
