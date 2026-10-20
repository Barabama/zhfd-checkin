# 智汇福大签到助手文档入口

> 隐私说明：2026-10-18 对 git 历史进行了重写，以移除测试 fixture 中出现的真实姓名、班级和定位地址。历史文档中出现的旧 commit hash 属于重写前引用；tag 名称（`v0.1.0`–`v0.3.2`）保持不变，指向重写后的对应提交。

此处是当前用户文档与工程文档的索引。第一次使用建议按顺序阅读：

1. [当前状态（唯一事实源）](./STATUS.md)：HEAD、tag、Release、CI、审计与未关闭项。
2. [新 AI 上手手册](./AI_ONBOARDING.md)：安全模型、代码地图、配置语义、陷阱与 recipes。
3. [快速开始](./QUICK_START.md)：从准备环境、首次设置，到首次单实例 dry-run。
4. GUI 左侧的“快速使用”：在程序内查看分步流程、结果解释和常见故障处理。
5. [Windows 安装、配置与运维](./OPERATIONS.md)：依赖路径、实例、数据目录、CLI 和计划任务。
6. [签到安全策略](./CHECKIN_SAFETY.md)：dry-run/live 边界、点击门禁、外勤限制和成功判定。

## 当前主题入口

| 文档 | 维护范围 |
|---|---|
| [STATUS.md](./STATUS.md) | 当前 HEAD/tag/Release/CI/审计状态；唯一事实源 |
| [AI_ONBOARDING.md](./AI_ONBOARDING.md) | 新 AI/新维护者上手：架构、门禁、代码地图、错误契约、隐私规则 |
| [快速开始](./QUICK_START.md) | 新用户首次运行流程、最小安全检查、dry-run 和结果确认 |
| [OPERATIONS.md](./OPERATIONS.md) | Windows 便携部署、TOML 配置、依赖发现、模拟器/实例/CLI 操作 |
| [CHECKIN_SAFETY.md](./CHECKIN_SAFETY.md) | 页面路由、签到业务类型、点击门禁、成功日期、证据边界 |
| [PRODUCT_AND_ARCHITECTURE.md](./PRODUCT_AND_ARCHITECTURE.md) | 产品职责、目标架构、Phase 状态与系统级待办 |
| [ENGINEERING_AND_RELEASE.md](./ENGINEERING_AND_RELEASE.md) | Cargo 项目布局、代码维护、CI/构建/打包与发布状态 |
| [adr/](./adr/) | 对冻结决策的修订记录（外勤 opt-in、BACK 回退、迁移持久化、自动登录提议） |
| [design/](./design/) | 尚未实现的功能设计稿（当前：自动登录/一键登录） |
| [audits/](./audits/) | 独立审计、修复复核及对应工程记录 |
| [archive/2026-10-11/](./archive/2026-10-11/) | 历史原件归档 |

## 历史原件映射

| 原文档 | 当前阅读入口 | 原件归档位置 |
|---|---|---|
| `PHASE0_SYSTEM_CONTRACT_2026-10-09.md` | PRODUCT_AND_ARCHITECTURE、CHECKIN_SAFETY | [architecture](./archive/2026-10-11/architecture/PHASE0_SYSTEM_CONTRACT_2026-10-09.md) |
| `PROJECT_IMPROVEMENT_BACKLOG_2026-10-09.md` | PRODUCT_AND_ARCHITECTURE、各 Phase 实施记录 | [architecture](./archive/2026-10-11/architecture/PROJECT_IMPROVEMENT_BACKLOG_2026-10-09.md) |
| `RUST_MIGRATION.md` | OPERATIONS、ENGINEERING_AND_RELEASE | [architecture](./archive/2026-10-11/architecture/RUST_MIGRATION.md) |
| `LDPLAYER_ENVIRONMENT.md` | OPERATIONS | [operations](./archive/2026-10-11/operations/LDPLAYER_ENVIRONMENT.md) |
| `EMULATOR_ENVIRONMENT.md` | OPERATIONS | [operations](./archive/2026-10-11/operations/EMULATOR_ENVIRONMENT.md) |
| `PROJECT_STATUS_2026-10-07.md` | PRODUCT_AND_ARCHITECTURE | [history](./archive/2026-10-11/history/PROJECT_STATUS_2026-10-07.md) |
| `CONTEXT.md` | PRODUCT_AND_ARCHITECTURE；仅作为早期上下文快照 | [history](./archive/2026-10-11/history/CONTEXT.md) |
| `WORKLOG_2026-09-18.md` | 早期工作历史 | [history](./archive/2026-10-11/history/WORKLOG_2026-09-18.md) |
| `SESSION_2026-09-21_SUMMARY.md` | 2026-09-21 会话快照 | [history](./archive/2026-10-11/history/SESSION_2026-09-21_SUMMARY.md) |
| `SESSION_2026-09-23_WRAPUP.md` | 2026-09-23 会话快照 | [history](./archive/2026-10-11/history/SESSION_2026-09-23_WRAPUP.md) |
| `SESSION_2026-09-24_HANDOFF.md` | 2026-09-24 会话快照 | [history](./archive/2026-10-11/history/SESSION_2026-09-24_HANDOFF.md) |
| `WINDOW_TEST_2026-09-22.md` | 窗口测试时间线与旧版本行为 | [history](./archive/2026-10-11/history/WINDOW_TEST_2026-09-22.md) |
| `WINDOW_TEST_2026-09-23.md` | 窗口测试时间线与旧版本行为 | [history](./archive/2026-10-11/history/WINDOW_TEST_2026-09-23.md) |
| `CLICK_VERIFY_2026-09-24.md` | 历史点击验证记录，不代表当前 release 验收 | [history](./archive/2026-10-11/history/CLICK_VERIFY_2026-09-24.md) |
| `IMAGE_ANALYSIS_2026-09-19.md` | 早期视觉样本记录 | [experiments](./archive/2026-10-11/experiments/IMAGE_ANALYSIS_2026-09-19.md) |
| 初版 `PHASE4_ENGINEERING_NOTES_2026-10-10.md` | 当前以 audits 下更新版为入口 | [audits snapshot](./archive/2026-10-11/audits/PHASE4_ENGINEERING_NOTES_2026-10-10.md) |
| 初版 `PHASE4_INDEPENDENT_AUDIT_PROMPT_2026-10-10.md` | 当前以 audits 下更新版为入口 | [audits snapshot](./archive/2026-10-11/audits/PHASE4_INDEPENDENT_AUDIT_PROMPT_2026-10-10.md) |
| `AUTOJS6_PCAPDROID_PLAN.md` | 独立手机端实验分支 | [experiments](./archive/2026-10-11/experiments/AUTOJS6_PCAPDROID_PLAN.md) |

## 当前审计证据

- [V0.3.1 独立审计报告（2026-10-18）](./audits/V0.3.1_INDEPENDENT_AUDIT_REPORT_2026-10-18.md)：审计对象为 pre-rewrite 的 `e135369` / `v0.3.1`（`9a66664` 为重写前 SHA）。结论为**不构成独立验收通过**，列出 P1（数据丢失、迁移持久化、计划任务路径、日期作用域、Setup 门禁、live 目标快照等）与设备/CI 证据缺口。
- [v0.3.0 独立审计报告（2026-10-12）](./V0.3.0_INDEPENDENT_AUDIT_REPORT_2026-10-12.md)：审计对象为 pre-rewrite 的 `c4b3bc4`，**不代表当前 master**。
- [Phase 4 独立审计报告](./audits/PHASE4_INDEPENDENT_AUDIT_REPORT_2026-10-11.md)：更早的 Phase 4 审计与 P2 修复复核，同样属于历史证据。
- [P2 修复说明](./audits/PHASE4_P2_REMEDIATION_2026-10-11.md)：实现侧变更范围。
- [审计提示词](./audits/PHASE4_INDEPENDENT_AUDIT_PROMPT_2026-10-10.md)：审计能力、设备安全边界和复核要求。
- [工程记录](./audits/PHASE4_ENGINEERING_NOTES_2026-10-10.md)：Phase 4 技术决策与剩余审计范围。

## 决策修订（ADR）

| ADR | 内容 |
|---|---|
| [ADR-0001](./adr/ADR-0001-field-work-live-opt-in.md) | 外勤签到 live 显式 opt-in（修订 D-08） |
| [ADR-0002](./adr/ADR-0002-swallowed-back-fallback.md) | BACK 被 App 吞掉时继续使用已确认签到页 |
| [ADR-0003](./adr/ADR-0003-recorded-data-root.md) | 持久化 GUI 迁移后的数据目录（修订 D-01） |
| [ADR-0004](./adr/ADR-0004-auto-login-opt-in.md) | 登录页「一键登录」显式 opt-in（已接受，默认关闭） |

## 设计稿

- [自动登录方案设计（2026-10-21）](./design/AUTO_LOGIN_DESIGN_2026-10-21.md)：现场证据、门禁、状态机、配置、结果契约与测试计划。状态为**已实现，待现场 live 验证**。

## 重复度盘点及归档说明

整理前按文档主题、章节结构、时间线和相同状态/命令进行盘点：没有发现字节完全相同的 Markdown 文件；重复主要是**摘要型内容重叠**：

- `CONTEXT`、多个 `SESSION`、`PROJECT_STATUS` 和 `RUST_MIGRATION` 都包含“一句话现状、文件职责、待办/恢复步骤”；
- `LDPLAYER_ENVIRONMENT` 与 `EMULATOR_ENVIRONMENT` 的模拟器/ADB 环境说明有交叉；
- Phase 0 契约、项目 Backlog、RUST migration 和 Phase 4 notes 多处重复架构目标、测试状态和未完成项；
- 各次 WINDOW/CLICK 记录本身是不可合并掉的实验时间线，虽主题相同但它们是历史证据，不作内容去重。

因此新文档整合“现在应该怎么理解/操作”的稳定内容；实验记录和阶段快照原样归档。历史原件中的相对链接是其写作时的路径语境，当前入口以本索引映射为准。仓库外用户提供的 `RUST_PLAN.md` 未导入、不改写。
