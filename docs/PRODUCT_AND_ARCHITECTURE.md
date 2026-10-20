# 产品目标、架构与当前状态

更新时间：2026-10-18（Asia/Shanghai）

## 产品边界

本项目是 Windows 上的签到辅助工具，通过 ADB 控制 LDPlayer 完成 App 前台确认、页面导航、签到状态观测，以及仅在显式授权后对普通签到执行一次点击。外勤签到默认只做 dry-run 观察；操作员显式启用 `runtime.allow_field_work_live` 后，才会进入独立的外勤 live 路径（ADR-0001）。它不负责登录凭据、验证码、账号身份、位置身份或学校服务端状态。

首要原则：**错误页/未知业务/未知视觉状态只能可解释地停止，不能猜测后继续点击。**

## 目标分层

```text
Presentation: GUI / CLI / Task Scheduler
        ↓ 共用应用服务
Application: RunCheckin / Diagnose / Configuration / ExportEvidence
        ↓
Domain: InstanceIdentity / PageRoute / CheckinMode / CheckinState / RunPolicy
        ↑
Infrastructure: ADB / LDPlayer / Windows process / TOML / files / image analysis
        ↓
Evidence: events.jsonl / result.json / screenshots / reports
```

GUI 是应用服务的展示和用户动作入口，不应复制实例发现、视觉策略、Profile 判定或点击授权。登录页一键登录已拆到 `rust/src/auto_login.rs`，通用页面模型在 `rust/src/page.rs`，前台检查在 `rust/src/foreground.rs`；其余运行编排仍集中于 `rust/src/main.rs`，分层是持续演进目标，不把目标文档误读成已经拆分完成。

## 当前阶段

| Phase | 实施状态 | 独立验收状态 |
|---|---|---|
| 0 契约与基线 | 契约文档已冻结 | 基线为 v0.1.0；后续决策以 ADR 修订 |
| 1 新机器可恢复 | Setup/Repair、数据根目录、迁移、原子保存与备份已实现 | Repair/迁移/GUI 交互仍缺黑盒验收 |
| 2 页面业务兼容 | Route/Mode/State、外勤 dry-run 与显式 opt-in live、成功日期解析已实现 | 单元/fixture 覆盖通过；窗口内真实设备 live/dry-run 仍待验收 |
| 3 GUI 信息架构 | 分组导航、Profile 排序、日志/事件证据浏览已实现 | 小窗口/缩放/长路径/大量日志仍未验收 |
| 4 工程补齐 | APK ZIP/XML 边界、结果契约、身份保留、port 语义、构建溯源已实现 | CI/release runner 已实际运行；完整设备级验收仍未完成 |

当前 `master` HEAD 为 `1b1931f`（`v0.3.2` 之后两个 docs-only 提交）；最新发布 tag `v0.3.2` 对应 `8d3f58e`，`v0.3.1` 对应 `a955d0a`。
当前状态与未关闭项以 [STATUS.md](./STATUS.md) 为准；独立审计见 [V0.3.1 独立审计报告](./audits/V0.3.1_INDEPENDENT_AUDIT_REPORT_2026-10-18.md)。

## 当前仍待完成的系统级事项

- 完成 `v0.3.2` 的独立审计（v0.3.1 审计中的 P1-1/2/3/6/7/8 已修复，P1-9 已由 ADR-0002 处理）；
- release workflow 绑定 tag/版本并依赖 CI（审计 P1-4 / P2-26）；
- 窗口内三实例 dry-run 与真实 live / 外勤 live 的设备级复验；
- GUI 低分辨率、高 DPI、窄窗口、长中文路径、迁移中断、计划任务交互黑盒验收；
- 架构拆分：把 `main.rs` 中的编排抽成应用服务，减少 GUI 对内部函数的直接依赖。

审计历史与精确测试矩阵见 [文档索引](./README.md) 的 audit 链接。
