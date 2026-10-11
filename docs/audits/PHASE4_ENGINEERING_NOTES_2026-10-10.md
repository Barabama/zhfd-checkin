# Phase 4 工程补齐记录

更新时间：2026-10-11（Asia/Shanghai）

## 实施边界

本阶段只完成工程代码、契约和发布治理材料；不在实现端执行测试、设备运行、GUI 验收或发布验收。最终验证由独立审计 agent 完成。

## 已落地

- APK binary XML / ZIP 解析器补充了真实 UiAutomator2 manifest fixture、ZIP 中央目录/本地头一致性、CRC、长度上限、EOCD 注释和字符串池边界处理。fixture 位于 [rust/src/testdata/uiautomator2_AndroidManifest.xml](../../rust/src/testdata/uiautomator2_AndroidManifest.xml)，固定断言 package=`com.github.uiautomator`、versionName=`2.4.0`、versionCode=`2004001`；另有 stored/deflated ZIP fixture 和畸形目录/属性反例测试代码。
- UiAutomator crate 暂不引入。现有 `adb shell uiautomator dump` 作为基础设施边界保留，原因是仓库已有可工作的命令协议，而 crate 版本/API/设备兼容性尚未形成稳定承诺。审计 agent 应把 shell 输出、编码、截断和设备 offline 作为黑盒兼容性范围。
- `ldplayer_reported_port` 替代含义不明确的 `adb_port`。该字段仅表示 `ldconsole list2` 原始报告值；实际连接身份仍以 `serial` 为准，避免把未经证明的字段当作 TCP ADB 端口。
- `RunResult.schema_version=2`，包含 run id、开始/结束时间、实例身份、屏幕几何、错误阶段、恢复建议和 artifact paths；配置能提供时在设备连接前 seed index/name/serial，随后在 LDPlayer 实例解析阶段更新，ADB 未就绪也尽可能保留身份。
- unknown 视觉状态使用连续帧跟踪器，不再把历史上不相邻的 unknown 累加为连续失败。
- CI/release 的既有 Windows 双 EXE、portable smoke、commit 嵌入和发布包路径保持不变；最终审计 agent 负责在干净 Windows 环境和 tag 模拟环境运行完整流程。

## 尚待独立审计

- 真 APK/ZIP、截断包、ZIP64/加密/数据描述符、CRC 错误、恶意长度和多 manifest 节点；
- `list2` 缺列、额外列、非数字报告端口、中文实例名和报告端口与 TCP serial 不一致；
- ADB/LDPlayer 发现失败时结果身份字段、全实例隔离及失败级联；
- GUI 小窗口、超长路径、100+ 日志、复制/导出、中文字体和日志证据泄露；
- 计划任务路径含空格、发布包干净目录和 GUI/CLI companion 启动。
