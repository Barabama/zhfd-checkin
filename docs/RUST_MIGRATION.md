# Rust 迁移状态（2026-09-25）

## 当前交付范围

Rust 第一阶段 CLI 已建立，Python 仍是行为参考基线。当前 CLI 使用：

- `image` + 手写 HSV、颜色覆盖率、文字行投影和连通区域搜索；
- ADB 命令实现截图、启动 App、点击、滑动、hierarchy dump、APK 安装；
- LDPlayer `ldconsole.exe` 自动发现、实例读取、启动和 profile 同步入口；
- `egui/eframe` 基础 GUI；
- `config.toml`、`logs/`、`captures/` 均位于 EXE 所在目录。

当前没有引入 `purecv`、`scirs2-vision`、`ndarray` 或 `numrs2`。现有四态算法不需要完整 OpenCV；后续只有在 profile 标定证明手写算法不足时才评估这些库。

## 命令

```powershell
zhfd-checkin.exe                 # GUI
zhfd-checkin.exe about
zhfd-checkin.exe diagnose
zhfd-checkin.exe profile list
zhfd-checkin.exe profile detect
zhfd-checkin.exe profile sync --id portrait_900x1600_d240 --confirm
zhfd-checkin.exe ui dump
zhfd-checkin.exe vision --image .\button.png
zhfd-checkin.exe run --dry-run
zhfd-checkin.exe run --live --confirm
zhfd-checkin.exe app install --apk D:\Downloads\zhfd.apk
zhfd-checkin.exe task create
zhfd-checkin.exe task query
zhfd-checkin.exe task delete
```

`profile sync` 会备份已发现的 LDPlayer 配置，修改目标实例分辨率/DPI，并重启实例；没有实现也不会修改 model、IMEI、账号、Cookie、Token 或定位数据。

## Profile

必须的八个雷电预设均已内置：

```text
landscape_1600x900_d240
portrait_900x1600_d240
landscape_1280x720_d240
portrait_720x1280_d240
landscape_1920x1080_d280
portrait_1080x1920_d280
landscape_960x540_d160
portrait_540x960_d160
```

工作区当前已有的 LDPlayer 实例实际报告为 `900x1600@320`，因此额外保留了兼容 profile `portrait_900x1600_d320`。随机 model、IMEI 和 ADB serial 不参与 profile 匹配。

只有已标定 profile 才允许 `--live`；未标定的八个新预设目前可被检测和 dry-run，但不会盲目真实点击。

## 已验证

```text
cargo check       PASS
cargo test        PASS
Python 四态回归    PASS（保留在原项目）
Rust fixture 四态  PASS
Rust ready crop    ready（4309 白字像素，双行结构）
Rust success crop  success（H≈61）
Rust release build PASS
```

当前 release CLI/GUI EXE 在本机生成于：

```text
rust/target/release/zhfd-checkin.exe
```

## 尚未完成

- 八个 profile 的真实截图标定；
- Rust 版本在四个 240 DPI profile 上的真实模拟器 dry-run；
- GUI 的更完整异步任务状态、诊断报告导出和同步确认对话框；
- UiAutomator Rust crate 的独立封装验证。目前使用 ADB 的 `uiautomator dump` 作为兼容方案；
- 干净 Windows 环境上的便携 EXE 验收。
