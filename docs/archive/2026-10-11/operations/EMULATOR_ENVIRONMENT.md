# 电脑端 Android 模拟器环境配置记录

> 目标：在 Windows 电脑上运行智汇福大测试实例，使用 Android Emulator 提供测试定位；主自动化仍由 Python + uiautomator2/OpenCV 执行。AutoJs6/PCAPdroid 手机端方案暂保留，不作为电脑模拟器的必需组件。

## 当前机器已确认

- Windows 10 Pro 64 位，系统版本 2009，构建 26200
- CPU：AMD Ryzen 5 5600，6 核 12 线程
- 物理内存：约 32 GiB
- CPU 虚拟化固件：已开启
- Windows 已检测到 Hyper-V hypervisor
- C 盘剩余约 72 GiB；D 盘剩余约 141 GiB
- 现有 ADB：`D:\Programs\platform-tools\adb.exe`
- Python 环境：`D:\Documents\Projects\check-in-zhfd\.conda`
- Android Studio 已安装：`C:\Program Files\Android\Android Studio`
- Android Studio 已安装；SDK 组件已安装到 `D:\Documents\Android\Sdk`，Android Studio 首次向导已完成或 SDK 已由向导配置

## 当前需要用户手动完成的一步

Android Studio 首次启动向导是图形交互，Agent 不直接读取桌面截图。请在向导中按以下选择：

1. 若询问导入旧设置，选择 **Do not import settings**。
2. 安装类型选择 **Custom**。
3. SDK 目录选择：`D:\Documents\Android\Sdk`。
4. 组件至少选择：
   - Android SDK
   - Android SDK Platform-Tools
   - Android SDK Build-Tools
   - Android SDK Command-line Tools
   - Android Emulator
5. 系统镜像优先选择带 **Google Play** 的 API 36 `x86_64` 镜像；如果列表没有 API 36，再选 API 35。
6. 接受 SDK License，开始下载。

Android Studio 官方安装页面和 Emulator 文档说明了 SDK、模拟器及虚拟设备的标准安装流程。

## 向导结束后的 Agent 验证

完成后只需回复“Android Studio 向导完成”，Agent 会通过命令行检查：

```powershell
Test-Path D:\Documents\Android\Sdk\platform-tools\adb.exe
Test-Path D:\Documents\Android\Sdk\emulator\emulator.exe
Test-Path D:\Documents\Android\Sdk\cmdline-tools\latest\bin\sdkmanager.bat
& D:\Documents\Android\Sdk\emulator\emulator.exe -accel-check
& D:\Documents\Android\Sdk\emulator\emulator.exe -list-avds
```

不会查看截图。只读取：

- AVD 名称；
- SDK 工具路径；
- 模拟器加速检查结果；
- ADB 设备列表；
- 设备系统属性；
- 前台包名和 UI hierarchy。

## 后续由 Agent 完成

1. 检查模拟器硬件加速。
2. 如果向导没有创建 AVD，创建一个 Google Play API 36 AVD。
3. 启动模拟器并等待 `sys.boot_completed=1`。
4. 安装/验证智汇福大 APK；如果实体机 APK 不在电脑，需要用户提供合法 APK 或从本人设备导出。
5. 设置测试位置；Android Emulator 的 Extended Controls 支持位置测试、单点位置和路线/GPX 等测试能力。
6. 将 Python 目标从实体机序列号改为 `emulator-5554`。
7. 运行 `probe.py`，只做页面和定位测试，不点击签到。
8. 再考虑电脑端抓包：优先 Wireshark/mitmproxy 观察，PCAPdroid 不作为模拟器运行的前置条件。

## 当前安全边界

- 模拟位置只用于授权测试和页面行为验证。
- 不伪造第三方签到请求、不修改响应、不绕过模拟器检测、Play Integrity、定位校验或设备完整性校验。
- 第一轮模拟器测试保持 dry-run，不执行真实签到点击。


## 2026-09-21 暂停状态

- WHPX 加速已验证可用。
- `platforms/android-36`、Emulator、Platform-Tools 和 Build-Tools 已安装。
- 当前没有 AVD。
- Google Play API 36 x86_64 system image 下载量较大，下载到约 57% 后按用户要求中断；下次可继续安装，或改用较小的 Google APIs x86_64 镜像。
- 命令行工具需使用 Android Studio 自带 JBR：`C:\Program Files\Android\Android Studio\jbr`。
