# Windows 安装、配置与运维

更新时间：2026-10-21（Asia/Shanghai）

> 第一次使用请先阅读 [QUICK_START.md](./QUICK_START.md)；本文件用于补充路径、实例、CLI、数据目录和长期运维细节。

## 便携部署

发布包包含 CLI 与 GUI 两个 EXE。首次运行在 EXE 同目录创建 `config.toml`、`logs/`、`captures/`、`reports/`、`backups/`。当前版本以 [STATUS.md](./STATUS.md) 为准；源码工作区构建不等同正式发布包。

配置默认跟随 EXE 所在目录，因此推荐将整个目录放在可写位置，而不是 `Program Files` 只读目录。便携包不应包含用户 config、日志、截图或模拟器状态。

## 数据目录迁移

GUI「首次设置 / 配置修复」页提供数据目录迁移。迁移会先把源配置备份到 `backups/`，复制 `logs/`、`captures/`、`reports/`、`backups/`，在目标写入并验证 `config.toml` 后才切换；源目录保留不删除。

迁移成功后会写入用户级指针 `%LOCALAPPDATA%\ZHFDCheckin\data-root.txt`（目标 `config.toml` 的绝对路径），后续启动优先使用该目录，避免重启后退回源目录（ADR-0003）。显式 `--config` 仍拥有最高优先级。删除该指针文件即可回滚到原有路径选择顺序。

损坏配置处于 Repair 状态时，普通保存和迁移都会被拒绝，避免把默认配置覆盖到原文件；请在 Repair 页显式完成修复后再操作。

## Windows TOML 路径

推荐 TOML literal string 或正斜杠，无需写双重反斜杠：

```toml
[emulator]
ldconsole_path = 'F:\leidian\LDPlayer14\ldconsole.exe'
adb_path = 'F:\leidian\LDPlayer14\adb.exe'

[[emulator.instances]]
name = '签到实例 0'
instance_index = 0
serial = ''
enabled = true
```

空的 `serial` 表示按实例 index 推导 TCP ADB serial；用户指定 serial 会覆盖该实例默认值。请勿把 `list2` 的 `ldplayer_reported_port` 当成 TCP ADB serial。

## 依赖发现与诊断

依赖查找优先级：显式配置 → EXE/便携目录 → LDPlayer 安装树/sibling ADB → 环境变量 → PATH → 匹配的注册表安装项 → 有限常见目录。GUI 配置页可扫描候选、显示来源并选择/保存；CLI `diagnose` 输出发现摘要。多安装版本应由用户选定，不依赖本机盘符假设。

主要只读/低风险命令：

```powershell
zhfd-checkin.exe about
zhfd-checkin.exe config path
zhfd-checkin.exe config show        # 账号与通知地址默认脱敏
zhfd-checkin.exe diagnose --instance-index 0
zhfd-checkin.exe instance list
zhfd-checkin.exe profile list
zhfd-checkin.exe profile detect --instance-index 0
zhfd-checkin.exe ui dump --instance-index 0   # 原始 hierarchy 可能含 PII
zhfd-checkin.exe login --instance-index 0     # 只探测登录页，零 tap
```

在自动发现与设备目标确认前，不要运行 `--all-instances`。`run --dry-run` 可能启动 App、导航或等待设备，但不应执行签到 tap；它仍属于设备动态操作，应由用户/审计者在目标环境决定。`--all-instances` 只运行配置中 enabled 的实例，且不能与 `--instance-index`/`--serial` 组合。

`run` 默认输出人类可读摘要；脚本解析请加 `--json`。变更型命令 `app install`、`task delete`、`profile sync`、`task create --live`、`run --live` 都要求显式 `--confirm`（clap 层 exit 2）。

`login` 是独立的会话建立命令：`login` 只探测；`login --confirm-login` 最多执行一次协议勾选 + 一次一键登录，不进入签到导航、不要求 Profile 标定。

## 多实例与 TCP serial

CLI 可用 `--instance-index N` 定位实例，`--serial SERIAL` 指定 TCP serial，`--all-instances` 依配置启用项顺序逐一执行。每个实例需独立确认 index/name/serial/Profile/log 路径。LDPlayer `list2` 报告端口单独显示为 `ldplayer_reported_port`，默认 TCP serial 规则是 `127.0.0.1:{5555 + 2*index}`，但显式配置优先。

## 运行窗口与任务

默认窗口 `21:00–23:59（Asia/Shanghai）`。GUI 和 CLI 都复用同一业务流程；默认 dry-run。计划任务推荐只创建 dry-run：

```powershell
zhfd-checkin.exe task create --name ZHFD-AutoCheckin-DryRun --all-instances
zhfd-checkin.exe task query --name ZHFD-AutoCheckin-DryRun
```

live 任务/设备运行不由文档默认授权；不要将 live 配置作为无人值守计划任务。

详细历史驱动、旧版本行为和端口采集记录保存在 [归档原件](./archive/2026-10-11/operations/LDPLAYER_ENVIRONMENT.md) 与 [迁移历史](./archive/2026-10-11/architecture/RUST_MIGRATION.md)。
