# ADR-0005：登录（会话建立）与签到（业务动作）分离

- 状态：**已接受并实现**
- 日期：2026-10-21
- 影响决策：`docs/V0.3.0_DECISIONS.md`「实施边界」；`docs/V0.3.0_FIRST_RUN_AND_RECOVERY_REQUIREMENTS.md` §10；[ADR-0004](./ADR-0004-auto-login-opt-in.md)
- 相关审计：[V0.3.3 AI 审计报告](../audits/V0.3.3_AUDIT_REPORT_2026-10-21.md)（DRY-01、SAFE-01、SAFE-02）

## 背景

ADR-0004 允许在 live 运行中执行一次一键登录，但登录仍嵌在 `run` 里，导致三个问题：

1. **dry-run 无法完成登录**：`authorize_auto_login` 在 `DryRun` 下只返回 `Ready`，`run --dry-run` 只能记录 `auto_login_ready`；
2. **唯一 tap 路径会继续签到**：`run --live --confirm` 在登录成功后会进入 `locate_or_navigate`，窗口内会执行真实签到点击，不是「只登录」；
3. **未标定预设无法登录**：live 在登录代码之前被 `uncalibrated_profile` 拦截，而登录点击坐标来自 hierarchy bounds，与 Profile 几何无关。

结论：**登录是会话建立（session setup），不是签到业务动作。** 它需要一个独立入口和独立的授权语义，而不是给 `run` 加开关。给 `run` 加登录开关会让「dry-run 永不点击」这条不变量出现例外，并使 `run` 同时承担两种职责。

## 决策

1. 新增一级命令 `zhfd-checkin login [--confirm-login] [--json] [target]`。
2. 语义分离：
   - `run` 仍然只负责签到；dry-run 仍然零 tap；
   - `login` 只负责在登录页完成一次一键登录，**不进入** `locate_or_navigate`、**不调用** `guarded_tap`、不读取或点击任何签到控件。
3. 授权：`login` 不使用 `--live --confirm`，而使用自己的 `--confirm-login`：
   - 无 `--confirm-login` → 只探测，输出 `auto_login_ready` 与 `login_only=true`，零 tap；
   - 有 `--confirm-login` → 最多一次协议复选框点击 + 一次登录点击。
4. 门禁复用 ADR-0004 的全部门禁（`allow_auto_login`、`accept_login_terms`、目标包前台、无外部窗口、唯一「一键登录」按钮、账号匹配、复选框视觉确认、点击前二次观测），只是把 `mode` 记为 `Live`。
5. **不要求 Profile 已标定**：登录坐标来自 hierarchy bounds。`login` 会记录检测到的 `profile_id`，但 `unknown_profile` / `uncalibrated_profile` 不阻断该命令。
6. 结果：`RunResult` 新增 `login_only: Option<bool>`；`login` 写的 `auto_login_*` 与 `clicked=false` 保持不变。
   - `clicked` 永远只表示签到按钮被点击；
   - 登录点击只体现在 `auto_login_tap_count` 与 `auto_login_tap` 事件。
7. `finalize_run` 对 `login_only == true` 且 `auto_login_succeeded == true` 的运行标记为 `success`；probe-only 的运行以 `auto_login_ready` 记录。
8. 不新增配置键；不使用 `runtime.mode`。

## 影响

- `run` 的行为完全不变（dry-run 仍零 tap，live 仍需 `--live --confirm`）。
- 会话过期后可以先用 `login` 恢复，再用 `run --dry-run` 观察签到页，形成可审计的两步流程。
- 未标定预设（1080x1920@480、720x1280@320、540x960@240）现在有可执行的登录验证路径。
- 新增一条设备 tap 路径，但仅限登录页，且一次运行最多两次点击（协议 + 登录）。
- 无人值守计划任务不受影响：`task create` 不暴露 `login`。

## 兼容性与回滚

- 新字段 `login_only` 为 `Option` + `#[serde(default)]`，`schema_version` 保持 3。
- 不改变任何现有命令的参数或退出语义；`login` 是新增命令。
- 回滚：把 `runtime.allow_auto_login` 设为 `false`（`login` 立即拒绝），或移除该命令。

## 未决 / 后续

- GUI 暂未提供「测试一键登录」按钮；如需，应按 `login` 的同一门禁实现，而不是复用 live 运行。
- live `login` 是否应受签到时间窗口约束：本 ADR 明确**不受**约束（会话建立与业务窗口无关），窗口检查只在 `run` 内。

## 现场验证（2026-10-21）

实例 index 0，720x1280@320，未标定 Profile：

1. `login --confirm-login --instance-index 0`
   - `status=success`、`login_only=true`
   - `auto_login_attempted=true`、`auto_login_succeeded=true`、`auto_login_tap_count=1`
   - `clicked=false`
   - 事件：consent tap (80,941) → one-click tap (360,746) → route=home
2. 随后 `run --dry-run --instance-index 0`
   - `page_route=checkin`、`checkin_mode=ordinary`、`checkin_state=ready`
   - `dry_run_ready=true`、`clicked=false`
   - `auto_login_attempted=false`、`auto_login_tap_count=0`

该验证证明：login 可以在未标定预设上完成会话建立，且不会把登录点击计入签到 `clicked`；随后 dry-run 能正常观察签到页。