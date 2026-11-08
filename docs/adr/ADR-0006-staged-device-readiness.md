# ADR-0006：设备就绪按阶段等待（实例出现 / VM 运行 / ADB / Android 引导）

- 状态：**已接受并实现**
- 日期：2026-10-29
- 影响决策：`docs/V0.3.0_DECISIONS.md` D-05（共享 preflight 的失败边界）；`runtime.startup_timeout_seconds` 的既有语义保持不变
- 相关审计：[V0.3.1 独立审计报告](../audits/V0.3.1_INDEPENDENT_AUDIT_REPORT_2026-10-18.md)（设备准备阶段的失败无法区分）

## 背景

计划任务与手动运行共用同一条设备准备路径，但它只有「一次性检查 + 一个 30 秒预算」：

1. `resolve_instance_target` 在 `ldconsole list2` 里找不到目标 index 就立即 `bail`；
   而完全关机的 LDPlayer 实例可能根本不出现在 `list2`。
   这一步发生在 `auto_launch` 被检查之前，因此 `emulator.auto_launch = true`
   对「实例还没出现」这一最需要它的场景完全无效。
2. `launch_wait` 只等到 `running=true`（VM 进程起来），不代表 guest Android 已引导；
   随后 `connect_adb_device` 成功即调用 `device.info()` → `wm size`，
   系统未就绪时整条路径塌成 `device_setup_failed`。
3. 全仓没有 `sys.boot_completed` 检查；`startup_timeout_seconds` 同时被
   启动命令、`adb connect`、等待 App 前台三处复用，任何一处变慢都表现为同一种失败。

现场表现（2026-10-21 22:48，远程部署机 `run --dry-run --instance-index 0`）：
停在 `device_setup` 并报 `LDPlayer 实例不存在: 0`，而实例只是尚未启动。

## 决策

1. **按阶段等待，每阶段独立预算、独立事件、独立错误码**：
   `instance_visible → instance_launch → adb_connect → boot`。
   每个阶段发一对 `wait_started` / `wait_finished` 事件（`stage`、`elapsed_ms`、
   `attempts`、`ok`，以及 `clicked=false`）。
2. **解析意图与要求存在分离**：`resolve_instance_intent` 只解析 index/serial/name，
   不要求实例已出现在 `list2`；是否等待、是否启动由调用方决定。
   - 运行路径（`run` / `login`）：允许启动时等待实例出现并启动；
   - `launch=false` 的路径（`diagnose` / `profile detect` / `ui dump` / GUI 探测 /
     `profile sync` 的前后读回）：不等待、不代为启动，立即失败。
     这些命令不应为一台自己不会被启动的 VM 空等。`profile sync` 自身的
     「退出 → 改配置 → 重启」是既有行为，与就绪等待无关。
3. **新增两个配置键，不改旧键语义**：
   - `runtime.instance_appear_timeout_seconds`（默认 30，使用处钳制 5–120）；
   - `runtime.boot_timeout_seconds`（默认 180，使用处钳制 30–600）。
   `runtime.startup_timeout_seconds` 继续表示「启动命令 → VM running」与
   「`adb connect` 重试」的预算。改旧键语义会让现场 `config.toml` 行为突变。
   两者的 `0` 都由 `Config::validate` 拒绝，上限在使用处钳制（与
   `location_timeout_seconds` 的既有模式一致），避免新增校验把老配置打进 Repair。
4. **引导判定**（三者同时满足）：
   - `getprop sys.boot_completed` 去空白后等于 `1`；
   - `getprop init.svc.bootanim` 为 `stopped`（读不到或为空视为未知，不阻塞）；
   - `wm size` 能被现有 `parse_size` 解析。
   引导期的单次 `adb shell` 失败只记录、不放弃——引导中 shell 本身可能不可用。
   三条读取由同一个 `AdbDevice::boot_snapshot()` 在一轮内取回，而不是分三个独立方法：
   分开调用会各自看到不同的时刻，超时信息也无法说清「最后一次探测缺的是哪一条」。
5. **新增稳定错误码**（`error_stage` 仍为 `device_setup`）：
   | code | 退出码 | 含义 |
   |---|---|---|
   | `instance_not_listed` | 5 | 实例未出现在 `list2`（且预算用尽，或本命令不会启动实例） |
   | `instance_launch_timeout` | 5 | 实例未在预算内进入 `running` |
   | `adb_connect_timeout` | 5 | ADB 未就绪 |
   | `boot_timeout` | 5 | Android 未完成引导 |
   | `instance_not_running` | 2 | 实例已列出但未运行，且本命令不会启动它（没有发生等待，因此不是超时） |
   `device_setup_failed`（退出码 2）保留给非等待类 I/O 故障（`ldconsole`/`adb` 无法执行）。
   错误码随类型化错误（`DeviceWaitFailure`）传播，不从 `error_stage` 反推。
6. **共享 preflight 的失败边界收窄**：`list2` 为空不再一律视为公共依赖失败。
   当 `emulator.auto_launch = true` 时，`--all-instances` 交给逐实例阶段等待并启动；
   只有 `auto_launch = false` 时仍按 D-05 报 `shared_preflight_failed`（全部实例 blocked）。
7. **`--all-instances` 保持串行**，每个实例独立预算；实例的准备耗时写入
   `result.json.device_ready_ms` 与 batch `summary.json.instances[].device_ready_ms`。

## 影响

- 不新增任何 tap 路径；`guarded_tap` / `authorize_page_action` / `authorize_click`
  及其调用顺序不变。等待只影响「设备是否就绪」，不放宽 route / mode / 前台 /
  窗口 / Profile / 连续 Ready / 点击前复核中的任何一条。
- dry-run 同样走完整等待（dry-run 不是离线预览），因此冷启动机器上 dry-run
  的实际耗时可能显著变长（最长约 `instance_appear + startup + boot`）。
- 失败时 `clicked=false` 由结构保证：等待全部发生在任何点击路径之前。
- `launch=false` 的命令在实例未启动时不再空等，失败信息与退出码仍可读（`instance_not_listed` 为 5、`instance_not_running` 为 2）。

## 兼容性与回滚

- 新增两个 `RuntimeConfig` 字段均带 serde 默认值，旧 `config.toml` 无需迁移；
  产物中新增字段 `device_ready_ms` 为 `Option` + `#[serde(default)]`，`schema_version` 保持 3。
- 旧版本二进制读新配置时会忽略未知键，并在下次保存时丢弃它们（既有行为，未改变）。
- 回滚方式：把 `boot_timeout_seconds` / `instance_appear_timeout_seconds` 设为最小值即可
  缩短新的等待；若需完全恢复旧行为，回退本次提交。

## 未决 / 后续

- GUI 暂未暴露这两个新键（只能编辑 `config.toml`）。
- 引导判定使用 `wm size` 解析而非 `sys.boot_completed` 之外的 API；若某构建不提供
  `init.svc.bootanim`，按未知处理（不阻塞）。
- **实例完全不在 `list2` 时无法代为启动**：`launch_wait` 会先重读 `list2`，因此只能启动「已列出但未运行」的实例。若实例缺失是因为 LDPlayer 服务本身未启动，等待后仍会得到 `instance_not_listed`（但已是可读的错误码，而不是笼统的 `device_setup_failed`）。若要在该场景下自动拉起，需要新增一条不依赖 `list2` 的直接 `launch` 路径；本次按规格未实现，因为规格明确要求「等待只观察、不启动」。
- 真实三状态验收（实例不存在 / 未运行 / 引导慢）需要模拟器现场操作，未经用户授权未执行。