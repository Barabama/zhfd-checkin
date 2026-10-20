# 自动登录（一键登录）方案设计

- 日期：2026-10-21
- 分支：`feat/auto-login`
- 状态：**已实现，待现场 live 验证**
- 关联决策：[ADR-0004](../adr/ADR-0004-auto-login-opt-in.md)（已接受）

---

## 1. 目标与非目标

### 目标

当 App 停留在登录页时，程序能够：

1. 用目标包自身的节点确认「这是登录页」；
2. 在**显式 opt-in** 且 `--live --confirm` 的前提下，完成一次**一键登录**；
3. 确认页面已离开登录页后，继续原有的签到导航与点击门禁；
4. 任何证据不足、状态不明或出现外部窗口时 fail-closed，不登录、不签到。

### 非目标（本版本明确不做）

- 不输入账号、密码、短信验证码；
- 不处理「其他账户登录」页面；
- 不保存任何凭据到 config、日志或环境变量；
- 不绕过《用户协议》《隐私协议》的同意语义；
- 不在 dry-run 中点击登录；
- 不在一次运行中重试登录（最多一次登录点击）；
- 不处理运行中途会话失效（只在启动导航前处理登录页）。

---

## 2. 现场证据（实例 index 0，2026-10-21）

设备：LDPlayer Android 14，900×1600，320 dpi；App `cn.edu.fzu.fdxypa` 1.17.4（versionCode 26091701）。

三个实例中，index 0 停留在登录页；index 1/2 已登录并停留在签到页。登录页 hierarchy（已脱敏）：

| 元素 | 节点证据 | bounds |
|---|---|---|
| 页面根 | `package="cn.edu.fzu.fdxypa"`，无外部窗口 | `[0,0][900,1600]` |
| 上次账号 | `content-desc="<学号>\n上次使用过的账号"` | `[72,136][828,1211]` |
| 一键登录 | `android.widget.Button`，`content-desc="一键登录"`，`clickable="true"` | `[72,846][828,952]` |
| 其他账户登录 | `content-desc="其他账户登录"`，`clickable="true"` | `[342,1002][558,1055]` |
| 同意复选框 | 无文本的 `View`，`clickable="true"`，位于同意文案左侧 | `[82,1116][120,1169]` |
| 同意文案 | `content-desc="登录即代表您已同意"` | `[120,1118][398,1180]` |
| 协议链接 | `《用户协议》`、`《隐私协议》` | 见 hierarchy |
| 注册链接 | `content-desc="点击注册"` | `[383,1485][517,1533]` |

视觉证据：

- 复选框为**未勾选**状态（白色圆底 + 浅灰描边，采样值 `#E9E9E9` / `#DDDDDD`）；
- 「一键登录」按钮为蓝色，采样值约 `RGB(13,148,255)`。

现有行为：`detect_page_route` 已经能识别 `PageRoute::Login`（命中 `一键登录` / `其他账户登录`），随后以 `login_required` 停止。**识别不是问题，缺的是登录动作。**

---

## 3. 安全模型

### 3.1 新增门禁（全部满足才允许登录点击）

| # | 门禁 | 说明 |
|---|---|---|
| G1 | `runtime.allow_auto_login = true` | 默认 `false`；旧配置缺键即为 false |
| G2 | `runtime.accept_login_terms = true` | 单独开关，表示操作员授权程序代勾选并同意协议 |
| G3 | `mode == Live && --confirm` | 复用现有 live 入口；dry-run 永不点击 |
| G4 | `route == Login` | 仅用目标包节点的 `一键登录` + `其他账户登录` 判定 |
| G5 | 目标 App 在前台，且无外部窗口 | 复用 `foreign_package` / focused window 证据 |
| G6 | 「一键登录」节点唯一、可点击、bounds 有效 | 多个候选视为歧义，拒绝 |
| G7 | 存在「上次使用过的账号」标签 | 无法确认账号时拒绝 |
| G8 | 配置了 `auto_login_account` 时必须精确匹配 | 防止登录到错误账号 |
| G9 | 同意复选框可定位，且点击后视觉确认已勾选 | 未确认勾选绝不点击登录 |
| G10 | 点击前重新观测仍是 Login + 同一账号 + 无 overlay | 与签到点击的 pre-tap 复核同构 |
| G11 | 一次运行最多一次登录点击 | 不重试、不循环 |
| G12 | 点击后只观察，不再点击任何控件 | 等待页面离开登录页 |

### 3.2 dry-run 行为

- 不点击复选框、不点击登录。
- 若 `allow_auto_login = true` 且登录页前置条件成立，结果写 `auto_login_ready = true`，随后仍以 `login_required` 结束（保持旧语义）。
- 若开关关闭，完全不探测，行为与当前版本一致。

### 3.3 同意语义

勾选协议是**代用户作出法律意义上的同意**。因此：

- 必须由独立开关 `accept_login_terms` 授权，不能由 `allow_auto_login` 隐式推导；
- GUI 用红色风险文案展示；
- 文档、ADR、结果事件都明确记录「程序代勾选协议」这一事实；
- 用户拒绝时返回 `auto_login_terms_not_accepted`，不点击。

---

## 4. 配置

```toml
[runtime]
# 是否允许自动登录（仅一键登录）。默认 false。
allow_auto_login = false
# 是否授权程序在登录页代勾选《用户协议》《隐私协议》。默认 false。
accept_login_terms = false
# 可选：期望的“上次使用过的账号”。为空表示不校验；只写本机配置，禁止提交。
auto_login_account = ""
# 一键登录后等待离开登录页的秒数，使用时钳制到 5-120，默认 30。
auto_login_timeout_seconds = 30
```

兼容性：四个字段全部 `#[serde(default)]`，旧配置无需迁移。

账号作用域：当前为**全局**配置，假定所有 enabled 实例使用同一账号。若未来需要每实例不同账号，再扩展 `[[emulator.instances]]`，本版本不做。

---

## 5. 状态机

```text
run 开始
  └─ App 启动 + 前台确认
       └─ auto_login_if_required()
            ├─ allow_auto_login = false            → NotNeeded（不探测）
            ├─ 观测 route != Login                 → NotNeeded
            ├─ 观测不可读 / 外部窗口 / 证据冲突      → 不登录，交回原流程
            ├─ mode = DryRun                       → Ready（只记录，不点击）
            ├─ 前置门禁失败                         → Refused(error_code)
            └─ mode = Live
                 ├─ 复选框已勾选 → 直接下一步
                 ├─ 未勾选 → 点击复选框 → 视觉复核
                 │    └─ 复核失败 → Refused(consent_unconfirmed)，不点登录
                 ├─ 点击前重新观测（仍 Login + 同账号 + 无 overlay）
                 ├─ 点击「一键登录」（本 run 唯一一次）
                 └─ 轮询等待离开登录页
                      ├─ Home / Services / Checkin → Performed(success)
                      ├─ overlay / 弹窗             → Refused(auto_login_blocked)
                      └─ 超时                       → Refused(auto_login_timeout)
       └─ 原 locate_or_navigate / 签到状态机继续
```

插入点：`run_inner_with_device` 在 `wait_for_package` 成功之后、`error_stage = "navigation"` 之前（当前 `main.rs:2051` 附近），调用 `auto_login_if_required`。无论返回 NotNeeded / Ready / Performed，随后都进入原有 `locate_or_navigate`；后者会重新观测页面，不复用登录前的证据。

不把登录逻辑塞进 `locate_or_navigate`：该函数已是高风险大函数，登录是独立动作，单独成函数便于测试和审计。

---

## 6. 识别与几何

### 6.1 登录页识别（复用）

`detect_page_route` 已使用目标包 `scoped_text` 中的 `一键登录` / `其他账户登录`。本方案不新增 page route，只新增「登录动作」。

### 6.2 一键登录按钮

- 在 `PageView.nodes` 中查找 `labels == "一键登录"`、`clickable == true` 的节点；
- 必须恰好 1 个；
- bounds 必须完全落在屏幕内，且宽高 > 0；
- 点击其中心。

### 6.3 同意复选框

当前 `PageNode` 只保存 `center`，建议补充 `bounds: Option<(u32,u32,u32,u32)>`，然后：

1. 找到 `labels` 含 `登录即代表您已同意` 的节点；
2. 找到 `clickable == true`、`labels` 为空、bounds 宽高均 ≤ 80、与同意文案垂直重叠、且位于文案左侧的节点；
3. 必须恰好 1 个；0 个或多个都拒绝（`auto_login_consent_not_found`）；
4. 点击其中心。

不使用固定坐标：实例分辨率可能变化，固定坐标会在其他 Profile 上误点。

### 6.4 复选框视觉复核

`vision` 新增一个最小函数：

```rust
pub fn accent_fraction(pixels: &RgbImage, rect: (u32,u32,u32,u32)) -> f32
```

规则：

- 统计矩形内「蓝色强调色」像素占比：`b >= 150 && b >= r + 40 && g >= 60`；
- 未勾选时接近 0；勾选后应明显上升；
- 判定阈值：占比 `>= 0.02` 且蓝色像素绝对数 `>= 40`；
- 点击前后各取一次截图，要求勾选后达到阈值，否则 `auto_login_consent_unconfirmed`；
- 如果点击前已经达到阈值，视为已勾选，直接跳过点击。

阈值来自 2026-10-21 的截图（未勾选区域为纯白/浅灰），但实现上仍需在另一台分辨率/主题下复测；阈值不满足时 fail-closed。

### 6.5 账号标签

- 查找 `labels` 含 `上次使用过的账号` 的节点；
- 取该标签中换行/`&#10;` 之前的前缀，trim 后作为账号标识；
- 只接受长度 4–32 的可见字符；
- 多个不同值 → 拒绝（歧义）；
- 日志与 result **只记录** `account_present` / `account_matched`，不记录账号原值；
- 若 `auto_login_account` 非空且不相等 → `auto_login_account_mismatch`，不点击。

---

## 7. 结果契约

新增字段（全部 `#[serde(default)]`，`Option`）：

| 字段 | 含义 |
|---|---|
| `auto_login_attempted` | 是否真正尝试登录（dry-run ready 为 false） |
| `auto_login_ready` | dry-run 下登录页且前置条件满足 |
| `auto_login_succeeded` | 是否确认离开登录页 |
| `auto_login_method` | 固定 `"one_click"` |
| `auto_login_account_configured` | 是否配置了期望账号 |
| `auto_login_account_matched` | 是否匹配；未配置时 `null` |
| `auto_login_tap_count` | 登录点击次数（0 或 1） |

新增 `error_code`：

| code | 退出码 | 含义 |
|---|---|---|
| `auto_login_terms_not_accepted` | 1 | 未授权代同意协议 |
| `auto_login_consent_not_found` | 1 | 找不到复选框节点 |
| `auto_login_consent_unconfirmed` | 1 | 点击后无法视觉确认已勾选 |
| `auto_login_button_ambiguous` | 1 | 一键登录节点缺失或不唯一 |
| `auto_login_account_mismatch` | 1 | 账号与配置不符 |
| `auto_login_account_unknown` | 1 | 无「上次使用过的账号」标签 |
| `auto_login_blocked` | 1 | 出现外部窗口/弹窗 |
| `auto_login_timeout` | 5 | 点击后未在超时内离开登录页 |
| `auto_login_not_confirmed` | 3 | 点击后页面层级不可读/证据冲突 |

`schema_version` 保持 3：新增字段全部是 `Option` + `#[serde(default)]`，旧消费者可忽略未知字段，旧结果也能被新二进制读取。若未来出现破坏性变更再单独升版。

事件（`events.jsonl`）：

```text
auto_login_probe        {route, overlay_package, account_present, clicked:false}
auto_login_refused      {reason, clicked:false}
auto_login_consent      {action:"already_checked"|"tapped"|"unconfirmed", clicked}
auto_login_tap          {method:"one_click", x, y, clicked:true}
auto_login_result       {succeeded, route, elapsed_ms}
```

事件中不得出现账号原值、协议文本全文或截图路径以外的 PII。

---

## 8. 代码改动清单

| 文件 | 改动 |
|---|---|
| `rust/src/config.rs` | 新增 4 个 runtime 字段 + 默认值 + 旧配置反序列化测试 |
| `rust/src/domain.rs` | 新增 `AutoLoginEvidence` / `AutoLoginDecision` / `authorize_auto_login`（纯函数，便于反例测试） |
| `rust/src/auto_login.rs` | 登录页选择器、协议复选框、`auto_login_if_required` / `perform_one_click_login`、轮询与结果写入 |
| `rust/src/main.rs` | `PageNode.bounds`、通用页面模型、调用点；不包含登录业务逻辑 |
| `rust/src/vision.rs` | `accent_pixels` / `consent_checkbox_checked`（比例阈值，带合成像素与缩放测试） |
| `rust/src/gui.rs` | Setup 两个开关 + 风险文案；结果面板显示 auto-login 状态；替换「程序不会自动登录」文案 |
| `rust/src/profile.rs` | `LDPLAYER_PRESETS` 预设目录；1080x1920@480、720x1280@320、540x960@240 的 DPI 对齐与占位几何 |
| `rust/src/logger.rs` | 无结构变更：`schema_version` 保持 3，新字段 `Option` + `serde(default)` |
| 测试数据 | 使用内联、脱敏的登录页 XML（账号为 `000000000`），不新增含 PII 的 fixture |

`run` 的 CLI 参数不变：登录由配置开关驱动，与 `allow_field_work_live` 的既有风格一致。

---

## 9. 测试计划

### 单元/无设备

1. 策略：开关关闭、dry-run、terms 未授权、overlay、账号不匹配、按钮歧义、复选框未确认 → 全部 `clicked=false`。
2. `PageView` 解析脱敏 fixture：找到 `一键登录`、复选框、同意文案、账号标签。
3. 账号解析：脱敏值、多个不同值、缺失值。
4. `accent_fraction`：合成「未勾选」与「已勾选」小图，阈值两侧均有断言。
5. 结果序列化：旧 JSON（无新字段）可读；新 JSON 字段完整。
6. `detect_page_route` 现有 Login 测试保持通过。

### 现场（需用户明确批准）

1. 在 index 0（当前未登录）：`run --dry-run --instance-index 0`，开启开关，预期 `login_required` + `auto_login_ready=true`，无点击。
2. `run --live --confirm --instance-index 0`：预期自动登录成功，随后完成签到；`auto_login_tap_count <= 1`。
3. 再次运行：已登录，预期 `auto_login_attempted=false`。
4. 关闭 `accept_login_terms` 重跑：预期 `auto_login_terms_not_accepted`，无点击。
5. 三个实例依次 dry-run，确认实例间无状态串扰。

---

## 10. 文档改动（实现时）

- `README.md` 安全边界：默认不自动登录 → 显式开启后仅一键登录；
- `CLAUDE.md` 红线与不变量：新增两个开关与「不保存凭据」；
- `docs/AI_ONBOARDING.md` §1/§4/§10；
- `docs/CHECKIN_SAFETY.md` 登录页章节；
- `docs/OPERATIONS.md` 配置表；
- `docs/QUICK_START.md` 首次使用；
- `docs/STATUS.md` 版本与验证成熟度；
- `docs/adr/ADR-0004-auto-login-opt-in.md` 状态从「提议」改为「已接受」。

---

## 11. 风险与未决问题

| 风险 | 影响 | 处理 |
|---|---|---|
| 一键登录依赖运营商/SIM，在 LDPlayer 中可能失败 | 功能不可用 | 现场验证；失败即 `auto_login_timeout`，不回退到密码 |
| 勾选协议是代用户同意 | 法律/合规 | 独立开关 + 红色警告 + ADR |
| App 在点击后弹出二次确认弹窗 | 登录不完成 | fail-closed，记录 `auto_login_blocked`；是否支持弹窗需现场证据后再决策 |
| 上次使用账号不是目标账号 | 登录错账号 | `auto_login_account` 强制匹配；未配置时记录风险 |
| 多个实例账号不同 | 功能不适用 | 本版本全局账号；后续按实例扩展 |
| 复选框视觉阈值受主题/分辨率影响 | 误判 | fail-closed；阈值需多分辨率复测 |
| 运行中途会话失效 | 不自动恢复 | 明确不在本版本范围 |

### 需要用户确认

用户已于 2026-10-21 确认：

1. 接受「程序代勾选《用户协议》《隐私协议》」；
2. 只支持一键登录，不做账号密码/SMS；
3. 配置期望账号并在登录前校验；
4. 允许在 index 0 上做一次 live 登录验证。

## 12. 实现记录

- `domain::authorize_auto_login` + `AutoLoginEvidence/AutoLoginDecision`：纯策略，覆盖关闭、dry-run、协议未授权、overlay、按钮歧义、账号未知/不匹配等反例。
- `auto_login::auto_login_if_required` / `perform_one_click_login`：探测、复选框视觉确认、点击前二次观测、最多一次登录点击、超时轮询。登录专用选择器与流程集中在 `rust/src/auto_login.rs`。
- 依赖分层：`page.rs`（PageView/PageNode/NodeBounds、路由识别、单轮观测）与 `foreground.rs`（前台包检查）是底层；`auto_login.rs` 依赖 `page`/`foreground`/`domain`/`config`/`vision`，不反向依赖 `main.rs`；`main.rs` 只负责编排与调用。
- `vision::accent_pixels` / `consent_checkbox_checked`：蓝色像素**比例**阈值（2%，绝对下限仅 4 px），随分辨率缩放。
- 复选框查找不再使用 80 px 绝对上限，改为相对同意文案的宽高比例；`auto_login.rs` 的缩放测试覆盖全部 8 个 LDPlayer 预设。
- `profile::LDPLAYER_PRESETS` 记录 8 个 LDPlayer 预设；1080x1920@480、720x1280@320、540x960@240 已对齐 DPI，几何比例仍是旧 DPI 占位值并保持 `calibrated=false`。自动登录本身不读 Profile 几何，但 live 仍受 `uncalibrated_profile` 门禁约束。
- `config::RuntimeConfig` 新增四个字段；GUI Setup 新增两个开关、期望账号与超时。
- 结果字段：`auto_login_attempted/ready/succeeded/method/account_configured/account_matched/tap_count`。
- 本地验证：`cargo fmt --check`、`cargo clippy -D warnings`、`cargo test --lib`（153 passed）。
- 现场验证（2026-10-21 00:23，index 0，live）：`auto_login_attempted=true`、`auto_login_succeeded=true`、`auto_login_tap_count=1`、账号匹配；事件顺序为 `auto_login_probe → consent(tapped) → tap(one_click) → result(route=home)`。复选框截图确认已勾选。
- 同一 run 随后在 `navigation` 阶段以 `route_candidate_unconfirmed` 停止。原因与登录无关：凌晨 00:25 已过 21:00–23:59 窗口，页面显示灰色“无法签到/无晚点名清单”，按钮不在 Profile 标定位置，视觉候选为 Unknown，导航按既有 fail-closed 规则停止。`clicked=false`。
- 回归（00:25，已登录的 index 0）：`auto_login_attempted=false`、`auto_login_tap_count=0`，说明登录页探测不会在非登录页误动作。
- 登录成功后新增“连续两次同一正向 route 才交接给导航”的稳定性等待，用于避免 Home→签到页过渡期的半渲染页面；该等待尚未在第二次真实登录中复测（实例已登录）。
- 现场证据来自核心登录流程的构建；之后又加入了“点击前用最新 observation 的复选框 bounds”和“任意 route 都检测登录弹窗”的收敛改动。这两项已通过编译与单元测试，但未再次现场登录复测。
- 预适配验证（2026-10-21 00:58，index 0，dry-run，设备已切换到 **720x1280@320**）：`profile_id=portrait_720x1280_d320` 精确匹配新预设；登录页探测 `auto_login_ready=true`、`auto_login_tap_count=0`、账号匹配。证明登录页选择器不依赖 900x1600 的几何，能适配新预设。
- 同一预设的 live 仍会在启动前被 `uncalibrated_profile` 阻止，因为 720x1280@320 的按钮几何仍是旧 DPI 占位值；这不是登录代码的耦合，而是 Profile 标定门禁。
- 尚未做：另一分辨率/主题下的复选框阈值复测；登录后完整签到成功的 live 验证（需要 21:00–23:59 窗口）；在 720x1280@320/1080x1920@480/540x960@240 上重新测量几何并标定。
