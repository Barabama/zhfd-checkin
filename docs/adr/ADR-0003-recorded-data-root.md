# ADR-0003：持久化 GUI 迁移后的数据目录

- 状态：已接受
- 日期：2026-10-18
- 影响决策：`docs/V0.3.0_DECISIONS.md` D-01（路径选择优先级）

## 背景

`ConfigStore::migrate_to_root` 会复制 config/logs/captures/reports/backups，
验证目标配置后返回新的 store。但它不改变磁盘上的候选顺序：源便携
`config.toml` 仍然存在，下一次启动时 loader 仍会优先命中源目录。

结果：迁移在当次进程内看似成功，重启后静默回到旧 root，新日志被写回旧目录。
这是审计报告 P1-2。

## 决策

1. 迁移成功、目标配置写入并验证后，**由 GUI 迁移入口**在用户级固定位置写入一个指针文件：
   `%LOCALAPPDATA%\ZHFDCheckin\data-root.txt`（内容为目标 `config.toml` 的绝对路径）。
   记录动作不放在 `ConfigStore::migrate_to_root` 内，避免库单元测试触碰真实用户目录；
   GUI 调用 `ConfigStore::record_current_root()` 完成记录。
2. `ConfigStore::load_with_config_path(None)` 的选择顺序调整为：
   1. 显式 `--config`；
   2. **已记录的数据目录**（指针存在、为绝对路径、且目标文件存在）；
   3. EXE 便携 `config.toml`；
   4. 已存在的 `%LOCALAPPDATA%\ZHFDCheckin\config.toml`；
   5. 尝试便携目录；
   6. LocalAppData。
3. 指针缺失、不可读、为相对路径或指向不存在的文件时**忽略**，回落到原候选链；
   指针本身永远不能阻止启动。
4. 迁移时若指针写入失败，迁移返回错误并明确提示「数据已复制，但重启后会回到旧目录」。
5. 处于 Repair 状态（`recovery_error.is_some()`）的 store 拒绝迁移与普通保存，
   避免把默认配置写进目标。

## 影响

- 迁移后重启会继续使用新目录，满足 D-01「迁移后切换 root」的意图。
- 便携包换机时：若用户级指针指向旧机器的路径且文件不存在，会被忽略，正常使用新便携配置。
- 显式 `--config` 仍拥有最高优先级，便于测试与多配置场景。
- GUI 的「配置来源」新增「已迁移的数据目录」标签。

## 兼容性与回滚

- 删除 `data-root.txt` 即回滚到原有候选顺序。
- 不删除源目录，符合迁移对话框「源目录保留未删除」的承诺。
