# 手机签到自动化 — 项目上下文

> 本文档为会话上下文存档，稍后在 `D:\Documents\Projects\check-in-zhfd` 继续工作。
> 日期：2026-09-17/18 创建

## 目标

给手机 app（"zhfd"）编写自动签到脚本：

1. 定时在某个时间段打开 app
2. 下划找到签到应用图标，点击进入
3. 等待页面加载、gps定位好
4. 判断签到按钮实际四态并执行/等待/报告：
   - **灰色“无法签到”** = 未到签到时段 → 等待重试
   - **蓝色“定位中...”** = 定位服务处理中 → 短暂等待
   - **蓝色“点击签到”** = 可以签到 → 点击
   - **绿色“签到成功”** = 已完成 → 结束

## 用户决策

- 路线：**方案 A — Windows 电脑 + Python + uiautomator2（USB 连真机）**
- 选择理由：用户希望 AI（Claude）直接辅助编辑、调试、截屏分析；电脑端方案可形成"执行→截图→AI 读图→改代码→重跑"闭环
- 页面技术栈：**不确定，需先探测**（原生 / H5 未知）→ probe.py 输出决定定位策略
- 备选方案 B：Auto.js6 纯本机（手机自跑，VS Code 插件推送脚本），不首选
- 调试完成后需要探究每天定时签到的方案

## 环境信息

- Windows 11 Pro，bash (Git Bash) shell
- platform-tools (adb) 位置：**D:\Programs\platform-tools\**（用户在编辑器中选中此路径）
- 计划任务参考：`schtasks /create /tn "AutoCheckin" /tr "python C:\checkin\checkin.py" /sc daily /st 07:55 /f`
- 依赖：`pip install uiautomator2 opencv-python numpy requests`，一次性 `python -m uiautomator2 init`
- 手机需开 USB 调试；锁屏无密码才能自动解锁，有密码需手动

## 文件

| 文件           | 用途                                                                                                                           |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| `checkin.py` | 主脚本：启动/解锁 app → 视觉找“我的服务”首项 → 进入晚点名签到 → 四态按钮识别 → 点击/等待/报告 |
| `probe.py`   | 探测脚本：dump_hierarchy 判断原生/H5；`--sample cx,cy,w,h` 采集按钮 HSV 标定三态阈值                                         |

## 待办（下次继续的步骤）

1. 手机连 USB，`adb devices -l` 确认显示 `402b184f device`。
2. 在真实签到窗口 21:30-23:59 运行一次 `ZHFD_DRY_RUN=1`，确认 ready 识别不会点击。
3. 确认定位中、点击签到、签到成功状态转换稳定后，再关闭 dry-run。
4. 最后配置 Windows 计划任务定时执行。

## 关键设计点（代码内已实现）

- find_icon：原生 `d(text="签到")` 优先，失败降级 OpenCV matchTemplate(阈值0.85) + 上滑最多6屏
- wait_stable：按钮区域连续两帧像素差 < 1.5 视为加载完成
- classify：按钮核心 HSV + 白字像素 — gray=无法签到；locating=蓝色定位中；ready=蓝色点击签到；success=绿色签到成功
- gray 状态窗口内每 2 分钟重试；每次关键步骤存 debug 截图
- 可选 Bark 推送（BARK_URL）

## 环境检查（2026-09-18）

- Conda 环境：`D:\Documents\Projects\check-in-zhfd\.conda`
- Python：3.12.14
- 已安装并通过导入测试：`uiautomator2 3.7.0`、`opencv-python 5.0.0.93`、`numpy 2.5.3`、`requests 2.34.2`
- `pip check`：通过；`scripts/checkin.py`、`scripts/probe.py`：编译通过
- ADB：`D:\Programs\platform-tools\adb.exe`，版本 1.0.41 / 37.0.1-15733141
- 2026-09-19 真实设备已恢复连接：`402b184f device`（PHY110）；ADB 服务可用
- 已增加 `requirements.txt`，并让脚本自动加入 ADB 路径、支持 `ZHFD_PKG` / `ANDROID_SERIAL` 环境变量；`probe.py` 在未设置包名时会保留当前前台 App，不再启动占位包名

## 收尾记录

详细工作总结见：docs/WORKLOG_2026-09-18.md。


## 2026-09-19 截图分析更新

- `data/images` 已通过视觉查看和 OpenCV 分析；详细记录见 `docs/IMAGE_ANALYSIS_2026-09-19.md`。
- 实际按钮不是简单三态：蓝色有“定位中...”和“点击签到”两种状态，绿色才是“签到成功”。
- `scripts/checkin.py` 已按四态状态机更新，并完成一次真实手机 dry-run；当前时间早于 21:30，未点击。

## AutoJs6 迁移实现（2026-09-19）

已新增 `autojs6/` 手机端实现骨架；默认 dry-run，PCAPdroid 默认关闭。四张真实状态 fixture 已通过离线回归测试。当前 PHY110 尚未安装 AutoJs6/PCAPdroid，下一步是在手机上导入并运行 `autojs6/probe.js`。详细记录见 `docs/AUTOJS6_PCAPDROID_PLAN.md`。

## 2026-09-20 手机端安装与导入

- 已确认设备 `402b184f` 在线，PHY110 / Android 16。
- AutoJs6：`org.autojs.autojs6`，版本 6.7.0；PCAPdroid：`com.emanuelef.remote_capture`，版本 2.0.1。
- 已将 `autojs6/` 复制到手机 `/storage/emulated/0/脚本/zhfd-autojs6/`，并预创建 `logs/`。
- AutoJs6 无障碍服务已通过 ADB 设置为启用，但手机实际设置页可能显示为未启用，需用户在设置中确认一次。
- 第一次运行 `probe.js` 暴露日志目录不存在问题，已修复 `autojs6/lib/logger.js` 并同步到手机；尚未完成截图权限授权和 probe 运行验证。
- 后续不再直接查看截图；调试优先使用 AutoJs6 日志、`probe.json`、UI hierarchy、ADB `logcat` 和文件列表。

## 2026-09-21 AutoJs6 首次运行

- 用户已安装 AutoJs6 6.7.0 和 PCAPdroid 2.0.1。
- `probe.js` 已在 AutoJs6 中打开并运行到 Android 16 截图授权页；当前需要用户手动点击“下一步”，选择“整个屏幕”（如果出现范围选择），再确认授权。
- 已修复 logger 的目录/文件创建问题并同步手机；后续不直接查看图像，只读取 JSON、事件日志、UI hierarchy、logcat 和文件列表。

## 2026-09-21 电脑 Android 模拟器环境

- 已安装 Android Studio 到 `C:\Program Files\Android\Android Studio`。
- Android Studio 已启动，等待首次启动向导。
- 计划 SDK 路径：`D:\Android\Sdk`；当前尚未确认 SDK/Emulator/AVD 是否安装完成。
- 机器：Windows 10 Pro 64 位、Ryzen 5 5600、约 32 GiB RAM；系统已检测到 Hyper-V hypervisor，CPU 虚拟化已开启。
- 详细配置步骤见 `docs/EMULATOR_ENVIRONMENT.md`。
- 按用户要求，模拟器阶段不直接查看截图，使用 ADB 状态、UI hierarchy、JSON、logcat 和文件列表调试。

## 2026-09-21 切换到雷电模拟器（LDPlayer 14）路线

- 用户已安装 LDPlayer 14（`E:\leidian\LDPlayer14`，v14.0.28.0，Android 14 / API 34），
  实例 0 为 900×1600 竖屏，已安装并登录智汇福大 v1.17.4。
- ldconsole/adb/uiautomator2 联动已验证；定位命令 `locate --LLI <Lng,Lat>` 已写入旗山校区坐标。
- 定位权限已通过 adb `pm grant` 授予 App。
- 详细记录见 `docs/LDPLAYER_ENVIRONMENT.md`；Android Studio AVD 路线暂停保留。
- 方案已定：模拟器=验证台，自动化引擎统一为 PC 端 Python+u2，模拟器内不装 AutoJs6；
  生产载体（模拟器/真机 USB/手机 AutoJs6）验证完再定。
- 关键发现：签到页是 App 内 WebView 的 H5（yzsxg.fzu.edu.cn），通过 JS bridge 申请定位权限；
  模拟器 hierarchy 的 content-desc 可读（真机不可读）；按钮 y 比例跨设备不同，
  `checkin.py` 已改为霍夫圆动态定位按钮；四态 fixture 回归 PASS；模拟器 dry-run（窗口外 gray）通过。

## 2026-09-22 窗口内验证（21:30-23:59）

- 窗口内 dry-run 完成：状态识别、定位注入到系统/App 原生层全部验证通过；
  但 H5 页面始终停在"定位中..."，未进入"点击签到"（未点击，符合边界）。
- 00:00 窗口一关页面立刻从 locating 转 gray，说明 H5 与服务器时序正常。
- 疑点集中在坐标系偏移（LD 交付值与写入值差约 1km）与高德 SDK 模拟器兼容。
- 21:32 的定时唤醒任务被消费但未实际执行——定时任务需要会话在触发时可用。
- 详见 `docs/WINDOW_TEST_2026-09-22.md`。

## 2026-09-21 项目结构整理

- Python 脚本从 `docs/` 移到 `scripts/`（checkin.py / probe.py / analyze_images.py / test_image_states.py）。
- 根目录调试采集归档到 `captures/phone|emulator|misc/`；脚本调试截图统一输出 `debug/`（已入 .gitignore）。
- 新增 `.gitignore`（排除 .conda/.vscode/debug/captures/data 等）并完成首次 git 提交。
- 文档内路径引用已同步；迁移后编译、四态 fixture 回归、模拟器 dry-run 均验证通过。

## 2026-09-23 会话收尾（今晚到此）

- 09-22 窗口验证完整收尾：链路全通、唯一卡点为 H5 停在"定位中"不转"点击签到"；
  疑点=坐标偏移/高德 SDK 模拟器兼容/坐标系混用，下次按 `docs/SESSION_2026-09-23_WRAPUP.md` 恢复。
- 定时唤醒教训：21:32 任务被消费但未执行——会话必须开着且空闲；无人值守改用 Windows 计划任务（待卡点解决后配）。
- 恢复入口：`docs/SESSION_2026-09-23_WRAPUP.md`（当前状态一句话、文件角色表、下次步骤）。

## 2026-09-22 签到窗口验证结果

- 23:43-23:59 窗口内在模拟器完成首次窗口内 dry-run：定位注入链路（ldconsole locate →
  gps_hal → App，71 次 fix 交付）与 H5 每 10 秒定位轮询均已验证；
  一度识别到蓝色 locating，但按钮最终停在灰色"无法签到"。
- 原因定位：注入坐标偏校区 1.84km（GCJ-02/WGS-84 换算漏乘 π）。
  正确 WGS-84 值 `119.188555,26.064324` 已注入并验证 gps_hal 收到；
  新增 `scripts/gcj2wgs.py` 做坐标系互转。
- 关键行为：H5 **只在签到窗口内**请求定位，窗口外 0 次轮询 → 完整复验需等下一个窗口。
- 详细记录见 `docs/LDPLAYER_ENVIRONMENT.md` 末节。

## 2026-09-21 暂停时最新进度

- 电脑 Android SDK 根目录已确认是 `D:\Documents\Android\Sdk`。
- 已安装并验证 Android Studio、SDK Platform 36、Emulator、Platform-Tools；WHPX 可用。
- 尚无 AVD，也没有完整 system image。Google Play API 36 x86_64 镜像下载曾进行到约 57%，随后按用户要求中断。
- 详细的当前状态、命令、恢复步骤见 `docs/SESSION_2026-09-21_SUMMARY.md`。
- 用户要求后续不直接查看图片；调试使用 UI hierarchy、日志、JSON、logcat、文件列表和数值图像分析。
