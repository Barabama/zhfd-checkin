# 独立审计 Agent 提示词：check-in-zhfd Phase 0–4 全量审计

你是本项目的**独立测试与审计 Agent**，不是只读代码审查员。你可以在共享工作区中编写反例测试、边界值测试、模糊/畸形 fixture、动态测试、级联测试、集成测试和必要的审计脚本，也可以运行 CLI/GUI/portable/release/设备流程。

## 任务目标

对 `<repo>` 当前工作区进行 Phase 0–4 全量独立审计，重点验证：

1. 新机器依赖发现、配置路径、Windows 子进程隐藏策略；
2. CLI/GUI 共享业务逻辑和 GUI 信息架构；
3. 多实例 index/name/serial/Profile/日志隔离；
4. 登录页、首页、服务页、普通签到页、外勤签到页、未知页路由；
5. 普通签到、外勤签到、未知模式的 fail-closed 行为；
6. 点击前二次确认、单次点击、点击后不重复点击；
7. success 文案、success 日期、配置时区、stale/date-unknown 结果语义；
8. RunResult schema v2、失败身份字段、error_stage/recovery_action/artifact_paths；
9. GUI Profile 稳定排序、日志筛选、横向/纵向滚动、长路径、复制、打开目录、结果导出；
10. APK ZIP/binary XML manifest 解析、真实 UiAutomator2 manifest fixture 和畸形输入；
11. `ldplayer_reported_port` 与实际 `serial` 的语义隔离；
12. portable package、CI/release workflow、干净 Windows 行为。

## 工作区和约束

- 工作区：`<repo>`
- 当前分支：`feature/phase1-environment-discovery`
- 当前存在未提交改动；**不得 reset、clean、checkout 覆盖或删除既有改动**。
- 不创建 tag，不 push，不发布 GitHub Release。
- 审计过程中允许新增测试、fixture、测试脚本和审计报告。
- 可以对发现的明确 correctness bug 做最小修复，但必须在报告中区分：原始发现、测试证据、修复内容；不要做无证据的大范围重构。
- 不修改账号、密码、验证码、定位身份或学校服务端状态。
- 不把外勤签到误判为普通签到，不放宽 calibrated/Profile/时间窗/前台包名/explicit confirmation 门禁。
- 真实设备流程优先使用诊断和 dry-run；只有在已有明确授权和安全条件同时满足时才可运行 live，不能为了审计主动点击真实签到。
- 测试产生的日志、截图、报告、临时 EXE、临时 config 必须放在临时目录或被 `.gitignore` 的目录，不要污染提交内容。

## 必须先做的基线记录

记录以下信息，但不要把“通过”误写成“已验收”：

```powershell
git status --short --branch
git diff --stat
git ls-files --others --exclude-standard
```

阅读并对照：

- `docs/README.md`（topic index and history mapping）
- `docs/PRODUCT_AND_ARCHITECTURE.md`
- `docs/OPERATIONS.md`
- `docs/CHECKIN_SAFETY.md`
- `docs/ENGINEERING_AND_RELEASE.md`
- `docs/audits/PHASE4_ENGINEERING_NOTES_2026-10-10.md`
- `docs/audits/PHASE4_P2_REMEDIATION_2026-10-11.md`
- `docs/archive/2026-10-11/architecture/PHASE0_SYSTEM_CONTRACT_2026-10-09.md`（frozen original）
- `docs/archive/2026-10-11/architecture/PROJECT_IMPROVEMENT_BACKLOG_2026-10-09.md`（frozen original）
- `rust/src/testdata/README.md`
- `rust/src/testdata/uiautomator2_AndroidManifest.xml`
- `.github/workflows/ci.yml`
- `.github/workflows/release.yml`

## 反例和边界值测试要求

不要只运行现有测试。至少新增并运行以下类别的测试：

### A. APK/ZIP/XML

- 真实 manifest fixture 正确读取 package/versionName/versionCode；
- ZIP 注释中包含伪 EOCD；
- 中央目录条目和 local header 文件名不一致；
- 中央目录/local header method、flags、CRC、压缩长度、未压缩长度不一致；
- 截断 EOCD、截断 central directory、截断 local header；
- ZIP64 标记、加密标记、跨磁盘标记、data descriptor；
- string pool UTF-8/UTF-16 长度溢出、偏移越界、空字符串、重复 manifest 节点；
- 过大 manifest 解压炸弹和整数溢出输入；
- 包名缺失、版本属性类型异常、未知属性类型。

### B. LDPlayer/ADB/多实例

- `list2` 缺少列、额外列、空列、非法 index/width/height/dpi/端口、中文名；
- `ldplayer_reported_port` 只作为 list2 报告字段，不能替代 `127.0.0.1:5555 + 2*index` 或显式 serial；
- index 与 serial 不对应时必须失败，不能静默回退；
- ADB offline/unauthorized/not found/connect 超时/空 screenshot；
- discovery 候选重复、大小写路径重复、多个版本、注册表异常输出、环境变量异常；
- `--all-instances` 的实例顺序、disabled 实例、单实例失败后后续实例是否继续、结果/日志是否隔离；
- ADB/LDPlayer 初始化失败前后 result.json 是否尽可能含 index/name/serial/run_id/error_stage。

### C. 页面/业务/视觉状态机

- 登录页不得自动登录；
- 首页、服务页、签到页、外勤页、未知页的正负样本；
- 首页下滑后入口才出现；服务页 tab 后入口才出现；入口不 clickable；入口标签损坏；
- 外勤页蓝/绿按钮不得进入 ordinary live tap；只有明确 hierarchy 证据才可输出 field_work_ready；
- ordinary ready、gray、locating、unknown 的连续/非连续帧序列；
- unknown、gray、locating、ready 交错时 unknown 阈值必须只按连续未知帧计算；
- ready 后页面切换、前台包变化、二次确认失败、重复成功帧、点击后再次 ready；
- success 绿色视觉但无成功文案、成功文案无日期、旧日期、跨午夜、当前时区日期；
- dry-run 永远不 tap，live 需要所有门禁，最多一次 tap。

### D. GUI/证据/发布

- 小窗口、窄窗口、低分辨率、中文字体；
- 左侧导航保持可用，主内容可滚动；
- Profile 先 calibrated，再稳定排序；
- 100+ 长日志、超长路径、筛选无结果、日志选择后预览切换；
- 复制路径/证据、打开目录、导出 result/sync/events、诊断报告脱敏；
- 错误结果的身份/阶段/恢复建议展示；
- 错误 config 不应让 GUI 无提示退出；
- portable staging 目录不依赖开发机私有路径；
- GUI/CLI companion、schtasks 路径含空格、没有控制台闪窗；
- release workflow 不把 config、logs、captures、reports、账号或调试产物打包。

## 动态/集成流程

在不执行 live 点击的前提下，尽可能运行：

```powershell
cargo fmt --manifest-path .\rust\Cargo.toml --all -- --check
cargo clippy --manifest-path .\rust\Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path .\rust\Cargo.toml
cargo build --release --manifest-path .\rust\Cargo.toml --bins
.\scripts\portable_smoke.ps1 -ExePath .\rust\target\release\zhfd-checkin.exe -FixturePath .\autojs6\fixtures\ready.png
node autojs6/validate_node.js
python -m compileall -q scripts
```

如果环境有真实 LDPlayer/ADB，可按实例逐一执行 diagnose、instance list、profile detect 和 dry-run；记录每个实例的 `index/name/serial/profile/log directory`。必须避免把外勤或未知页当作普通签到成功。

## 结果输出

最后生成一份 Markdown 审计报告，至少包括：

1. 执行环境、分支、commit、工作区初始/最终状态；
2. 测试矩阵和每个命令的结果；
3. 新增的测试/fixture/脚本文件；
4. 失败用例的最小复现步骤和实际输出；
5. 按 `[P0]`/`[P1]`/`[P2]`/`[P3]` 排序的 findings，附绝对路径、行号/函数、影响场景和修复建议；
6. 明确区分“代码 bug”“测试环境问题”“未验证”；
7. 是否发现会导致未授权 tap、实例串用、错误成功、凭据泄露或发布污染的问题；
8. 若做了最小修复，列出修改文件和修复前后反例；
9. 给出 Phase 4 是否可以进入独立验收通过的结论，但不要创建 tag 或发布。

审计的核心原则：**优先发现真实风险，不为通过率编写迎合性测试；只要安全不变量、实例隔离、结果可信度或发布边界存在疑问，就保留反例证据。**


## 2026-10-11 P2 follow-up re-audit

初次审计报告 `docs/audits/PHASE4_INDEPENDENT_AUDIT_REPORT_2026-10-11.md` 后实现端补了两个待复核的 P2 remediation，详情见 `docs/audits/PHASE4_P2_REMEDIATION_2026-10-11.md`。请在完整复审之外增加：

1. 无效 TOML 下 standalone GUI 是否显示原生可见恢复对话框，内容包含配置路径/修复方法/log 路径；关闭后 GUI 进程是否返回非零；日志写入失败时是否仍有可见错误；正常启动路径是否未受影响。
2. `cargo metadata`、CLI/GUI About 与 release package metadata 是否均为 `0.2.0`；package 文件名/README 版本是否一致；不得创建 tag、push 或 GitHub Release。
3. 回归审查 GUI 错误对话框实现是否依赖 console、可能 panic、丢失错误细节或把 config 自动覆盖。

这两项只有反例/动态复核通过并在报告中附证据，才可标为 closed；source diff 已修改本身不等于关闭。
