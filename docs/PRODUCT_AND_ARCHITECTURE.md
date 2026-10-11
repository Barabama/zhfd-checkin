# 产品目标、架构与当前状态

更新时间：2026-10-11（Asia/Shanghai）

## 产品边界

本项目是 Windows 上的签到辅助工具，通过 ADB 控制 LDPlayer 完成 App 前台确认、页面导航、签到状态观测，以及仅在显式授权后对普通签到执行一次点击。它不负责登录凭据、验证码、账号身份、位置身份或学校服务端状态。

首要原则：**错误页/未知业务/未知视觉状态只能可解释地停止，不能猜测后继续点击。** 外勤签到是独立业务模式，目前仅允许 dry-run 观察。

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

GUI 是应用服务的展示和用户动作入口，不应复制实例发现、视觉策略、Profile 判定或点击授权。实现当前仍将大量编排集中于 `rust/src/main.rs`，分层是持续演进目标，不把目标文档误读成已经拆分完成。

## 当前阶段

| Phase | 实施状态 | 独立验收状态 |
|---|---|---|
| 0 契约与基线 | 契约文档已冻结 | 已有 2026-10-09 基线；它描述的是 v0.1.0 基线 |
| 1 新机器可恢复 | 依赖发现/候选报告/GUI 选择切片已实现；配置 Repair/reload 仍有差距 | 新机器无路径发现和无闪窗尚未在目标机验收 |
| 2 页面业务兼容 | Route/Mode/State、外勤 dry-run、日期结果结构已实现 | 最终审计报告的单元/fixture 覆盖通过；真实 LDPlayer 页面流未完成 |
| 3 GUI 信息架构 | 左侧分组导航、Profile 排序、日志证据浏览已实现 | 完整小窗口/缩放/交互验收尚未完成 |
| 4 工程补齐 | APK ZIP/XML 边界、结果契约、身份保留、port 语义等已实现 | 两项 P2 在 [审计报告](./audits/PHASE4_INDEPENDENT_AUDIT_REPORT_2026-10-11.md) 中复核关闭；整体仍待真实设备、多实例、GUI 全路径和 CI/release runner 验收 |

当前工作分支 `feature/phase1-environment-discovery` 的实现 checkpoint 为 `bbc53b7`。最近一次审计后续报告仍是未提交工作区材料；提交状态应以 `git status` 为准。

## 当前仍待完成的系统级事项

- 新机器上自动发现雷电多版本和对应 ADB 的实际安装布局；
- 错误 TOML 的启动恢复页面/GUI 配置重载工作流（启动错误对话框仅解决启动失败可见性，不等于 Repair UI）；
- LDPlayer 真实设备、多个实例顺序/失败级联/日志隔离；
- 低分辨率和真实 GUI 长路径/大量日志交互；
- Windows CI/tag workflow 的远端验证及干净用户验收。

审计历史与精确测试矩阵见 [文档索引](./README.md) 的 audit 链接。
