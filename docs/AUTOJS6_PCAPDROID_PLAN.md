# 智汇福大手机端 AutoJs6 + PCAPdroid 实施记录

## 已实现

- 新增 `autojs6/` 手机端脚本骨架。
- `probe.js`：请求截图、启动智汇福大、保存截图和 JSON 探测结果，不点击签到。
- `main.js`：唤醒/解锁、启动 App、判断是否已经在签到页、视觉进入“我的服务”首项、运行四态状态机。
- 四态识别：`gray`、`locating`、`ready`、`success`。
- 使用真实截图得到的按钮 HSV、彩色覆盖率和白字比例阈值。
- 默认 dry-run；只有显式把 `config.js` 中 `runtime.dryRun` 改为 `false` 才会点击。
- `pcapdroid.js`：封装 PCAPdroid `CaptureCtrl` 的 start/stop/get_status Intent，默认关闭。
- 日志、截图、`result.json` 保存到手机 `logs/<timestamp>/`。
- 保留 Python 版本作为离线参考和回归基线。

## 当前设备基线

- App：智汇福大 v1.17.3
- 包名：`cn.edu.fzu.fdxypa`
- 设备：PHY110，1440×3168
- 签到窗口：21:00—23:59
- 按钮中心比例：`[0.5, 0.5543]`
- 首页入口中心比例：`[0.1417, 0.5919]`

## 实施顺序

1. 在手机安装并配置 AutoJs6。
2. 运行 `probe.js`，确认截图权限、启动、日志和 `probe.json` 正常。
3. 在非签到时段运行 `main.js`，确认灰色状态只等待/报告，不点击。
4. 在真实签到窗口先保持 `dryRun=true`，确认 `ready` 识别准确且绝不点击错误状态。
5. 手动使用 PCAPdroid 完成一次完整观察，过滤 `cn.edu.fzu.fdxypa`。
6. 确认 PCAPdroid API 权限后，将 `pcapdroid.enabled` 改为 `true`，验证 start/stop 和 PCAP 文件。
7. 只有完成 dry-run 和抓包基线后，才将 `runtime.dryRun` 改为 `false`。
8. 完成真实签到后重新进入签到页，记录 `server_state_reverified` 的人工/脚本复核结果。

## 验收

- 四张 fixture 截图分别得到预期状态。
- 黑屏/加载页不会被判为灰色。
- `unknown` 不会触发点击。
- App 已在签到页时不会重复找首页入口。
- PCAPdroid 失败不会阻止签到流程。
- Token、Cookie、个人定位地址不写入普通日志。
- 首次正式运行需要人工观察手机屏幕，不配置无人值守计划任务，直到成功复核。

## 后续机型

新增机型只增加 `autojs6/profiles/<model>.json`，不复制主流程。优先使用 OCR、区域/圆形检测和比例坐标，profile 仅覆盖屏幕尺寸、入口坐标、解锁手势和 OCR 缩放等差异。

## 抓包边界

PCAPdroid 仅做授权设备上的网络观察和故障诊断；不实施请求伪造、重放、响应注入或绕过签到服务校验。

## 当前实现状态（2026-09-19）

- 已创建 `autojs6/` 目录及 `config.js`、`main.js`、`probe.js`、`lib/`、`profiles/PHY110.json`、`fixtures/`。
- 默认 `dryRun=true`，首次导入 AutoJs6 不会执行签到点击。
- 已从 `data/images` 复制灰色、定位中、点击签到、签到成功四张 fixture，并通过 Python 参考分类器回归测试。
- 已通过 Node 语法检查和结构 smoke test；这些检查不等于已经在 AutoJs6 运行时完成验证。
- 2026-09-20 已确认 PHY110 安装 AutoJs6 6.7.0（`org.autojs.autojs6`）和 PCAPdroid 2.0.1（`com.emanuelef.remote_capture`）；脚本已复制到 `/storage/emulated/0/脚本/zhfd-autojs6/`。
- PCAPdroid 集成默认关闭；先手动抓包，确认权限和输出目录后再将 `config.js` 中的 `pcapdroid.enabled` 改为 `true`。

下一步：在 AutoJs6 中打开 `/脚本/zhfd-autojs6`，先运行 `probe.js` 并按系统提示授予截图权限；随后在非签到时段运行 `main.js`。首次运行曾因 `logs` 目录不存在报错，已预创建并修复 logger。

## 2026-09-21 运行验证进度

- 已通过 AutoJs6 6.7.0 从 `/脚本/zhfd-autojs6` 打开 `probe.js`。
- 已修复 AutoJs6 日志目录创建和日志文件写入问题，并同步 `lib/logger.js` 到手机。
- 当前运行已进入 Android 16 MediaProjection 系统授权页，UI 文本为“要开始使用AutoJs6录制或投放吗？”、“下一步”、“单个应用”、“取消”。
- 需要用户手动完成截图授权；授权前脚本不会继续启动智汇福大。
- 按用户要求，后续不直接查看图片；只读取 `probe.json`、`events.log`、UI hierarchy、前台包名、logcat 和文件列表。
