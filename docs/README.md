# 智汇福大签到助手文档入口

此处是当前用户文档与工程文档的索引。第一次使用建议按顺序阅读：

1. [快速开始](./QUICK_START.md)：从准备环境、首次设置，到首次单实例 dry-run。
2. GUI 左侧的“快速使用”：在程序内查看分步流程、结果解释和常见故障处理。
3. [Windows 安装、配置与运维](./OPERATIONS.md)：依赖路径、实例、数据目录、CLI 和计划任务。
4. [签到安全策略](./CHECKIN_SAFETY.md)：dry-run/live 边界、点击门禁、外勤限制和成功判定。

## 当前主题入口

| 文档 | 维护范围 |
|---|---|
| [快速开始](./QUICK_START.md) | 新用户首次运行流程、最小安全检查、dry-run 和结果确认 |
| [OPERATIONS.md](./OPERATIONS.md) | Windows 便携部署、TOML 配置、依赖发现、模拟器/实例/CLI 操作 |
| [CHECKIN_SAFETY.md](./CHECKIN_SAFETY.md) | 页面路由、签到业务类型、点击门禁、成功日期、证据边界 |
| [PRODUCT_AND_ARCHITECTURE.md](./PRODUCT_AND_ARCHITECTURE.md) | 产品职责、目标架构、Phase 状态与系统级待办 |
| [ENGINEERING_AND_RELEASE.md](./ENGINEERING_AND_RELEASE.md) | Cargo 项目布局、代码维护、CI/构建/打包与发布状态 |
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

- [独立审计报告](./audits/PHASE4_INDEPENDENT_AUDIT_REPORT_2026-10-11.md)：包含完整测试矩阵、原始 findings 与 P2 修复后复核。当前两项 P2 已有独立证据关闭；Phase 4 整体仍因没有真实 LDPlayer、多实例级联、完整 GUI 视觉和 GitHub workflow 运行而未完成验收。
- [P2 修复说明](./audits/PHASE4_P2_REMEDIATION_2026-10-11.md)：实现侧变更范围。
- [审计提示词](./audits/PHASE4_INDEPENDENT_AUDIT_PROMPT_2026-10-10.md)：审计能力、设备安全边界和复核要求。
- [工程记录](./audits/PHASE4_ENGINEERING_NOTES_2026-10-10.md)：Phase 4 技术决策与剩余审计范围。

## 重复度盘点及归档说明

整理前按文档主题、章节结构、时间线和相同状态/命令进行盘点：没有发现字节完全相同的 Markdown 文件；重复主要是**摘要型内容重叠**：

- `CONTEXT`、多个 `SESSION`、`PROJECT_STATUS` 和 `RUST_MIGRATION` 都包含“一句话现状、文件职责、待办/恢复步骤”；
- `LDPLAYER_ENVIRONMENT` 与 `EMULATOR_ENVIRONMENT` 的模拟器/ADB 环境说明有交叉；
- Phase 0 契约、项目 Backlog、RUST migration 和 Phase 4 notes 多处重复架构目标、测试状态和未完成项；
- 各次 WINDOW/CLICK 记录本身是不可合并掉的实验时间线，虽主题相同但它们是历史证据，不作内容去重。

因此新文档整合“现在应该怎么理解/操作”的稳定内容；实验记录和阶段快照原样归档。历史原件中的相对链接是其写作时的路径语境，当前入口以本索引映射为准。仓库外用户提供的 `RUST_PLAN.md` 未导入、不改写。
