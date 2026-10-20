# zhfd-checkin

Windows 便携式「智汇福大晚点名签到」辅助工具。Rust CLI + GUI，通过 ADB 控制 LDPlayer；默认 dry-run，live 点击必须在用户显式授权后通过全部门禁。

## 文档入口

| 文档 | 内容 |
|---|---|
| [docs/STATUS.md](docs/STATUS.md) | 当前 HEAD / tag / Release / CI / 审计状态（唯一事实源） |
| [docs/AI_ONBOARDING.md](docs/AI_ONBOARDING.md) | 新 AI / 新维护者上手：架构、门禁、代码地图、配置、陷阱 |
| [docs/QUICK_START.md](docs/QUICK_START.md) | 第一次使用：环境、首次设置、单实例 dry-run |
| [docs/OPERATIONS.md](docs/OPERATIONS.md) | 安装、配置、实例、CLI、数据目录、计划任务 |
| [docs/CHECKIN_SAFETY.md](docs/CHECKIN_SAFETY.md) | 页面路由、点击门禁、成功判定、外勤限制 |
| [docs/README.md](docs/README.md) | 完整文档索引与权威层级 |

## 快速构建

```powershell
cd rust
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --lib
cargo build --release --bins
```

产物：`rust/target/release/zhfd-checkin.exe`、`zhfd-checkin-gui.exe`。

## 安全边界

- 默认不自动登录，不输入或保存账号/密码/验证码；只有同时显式开启 `runtime.allow_auto_login` 与 `runtime.accept_login_terms` 的 live 运行，才会在登录页执行一次「一键登录」（使用 App 记住的上次账号）。
- 不伪造定位、设备身份或账号。
- dry-run 只观察，不点击签到按钮；live 必须 `--live --confirm` 且通过 route/mode/前台/窗口/Profile/连续 Ready/点击前复核。
- 未知页面、证据冲突、overlay、外勤未显式 opt-in 时一律 fail-closed。

修改代码前请先读 [CLAUDE.md](CLAUDE.md) 与 [docs/AI_ONBOARDING.md](docs/AI_ONBOARDING.md)。
