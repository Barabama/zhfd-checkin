# 2026-09-21 会话暂停总结：电脑 Android Emulator 环境

## 用户当前决定

- 暂停 AutoJs6/实体手机端的继续调试。
- 改为先在 Windows 电脑上配置 Android Emulator，用于测试智汇福大页面、定位行为和电脑端自动化。
- 遵守用户要求：后续不直接查看图片；使用 ADB 状态、UI hierarchy、JSON、logcat、文件列表和 OpenCV 数值分析。
- 当前不执行真实签到，不实现请求伪造、响应注入、重放或绕过定位/设备校验。

## 电脑环境

- 工作区：`D:\Documents\Projects\check-in-zhfd`
- Windows 10 Pro 64 位，构建 `26200`
- CPU：AMD Ryzen 5 5600，6 核 / 12 线程
- 内存：约 32 GiB
- CPU 固件虚拟化：已开启
- Windows Hyper-V hypervisor：已检测到
- Android Emulator 加速：
  ```text
  WHPX(10.0.26200) is installed and usable.
  ```
- D 盘剩余空间约 137.9 GiB（截至本次检查）

## Android 工具安装状态

### Android Studio

已安装：

```text
C:\Program Files\Android\Android Studio
```

其自带 Java：

```text
C:\Program Files\Android\Android Studio\jbr
OpenJDK 25.0.3
```

### Android SDK

正确 SDK 根目录：

```text
D:\Documents\Android\Sdk
```

已确认存在：

```text
D:\Documents\Android\Sdk\platform-tools\adb.exe
D:\Documents\Android\Sdk\emulator\emulator.exe
D:\Documents\Android\Sdk\cmdline-tools\latest\bin\sdkmanager.bat
D:\Documents\Android\Sdk\cmdline-tools\latest\bin\avdmanager.bat
```

已安装 SDK 包：

```text
platform-tools 37.0.1
emulator 37.1.11
platforms/android-36 2.0.0
platforms/android-37.0 2.0.0
build-tools/36.0.0
build-tools/37.0.0
cmdline-tools/latest 23.0.0
```

### AVD

当前没有已创建的 AVD：

```text
emulator -list-avds
# 无输出
```

当前没有完整安装的 system image：

```text
D:\Documents\Android\Sdk\system-images\android-36\google_apis_playstore\x86_64\package.xml
# 不存在
```

## 本次安装尝试

1. 已成功安装 API 36 平台包：
   ```text
   platforms/android-36
   ```
2. 尝试安装 Google Play API 36 x86_64 system image：
   ```text
   system-images/android-36/google_apis_playstore/x86_64
   ```
3. 下载包体约 1.8 GiB；因用户要求暂停，已中断下载。
4. 中断时下载进度约 57%，但最终没有形成完整可用的 system image，也没有创建 AVD。
5. 下次可以重新安装该包；SDK Manager 会从已有缓存/临时状态继续或重新下载。

注意：新版 Android CLI 对旧式带分号的 sdkmanager 参数处理异常，本次使用下面的斜杠形式才正确：

```powershell
sdkmanager.bat --sdk_root=D:\Documents\Android\Sdk platforms/android-36
sdkmanager.bat --sdk_root=D:\Documents\Android\Sdk system-images/android-36/google_apis_playstore/x86_64
```

运行命令行工具时必须设置 Android Studio 自带 Java：

```powershell
$env:JAVA_HOME='C:\Program Files\Android\Android Studio\jbr'
$env:PATH="$env:JAVA_HOME\bin;D:\Documents\Android\Sdk\platform-tools;D:\Documents\Android\Sdk\emulator;D:\Documents\Android\Sdk\cmdline-tools\latest\bin;$env:PATH"
```

## 原实体手机/AutoJs6 状态

- 手机序列号：`402b184f`
- 设备：PHY110 / Android 16 / 1440×3168
- AutoJs6：`org.autojs.autojs6`，6.7.0
- PCAPdroid：`com.emanuelef.remote_capture`，2.0.1
- AutoJs6 脚本已复制到：
  ```text
  /storage/emulated/0/脚本/zhfd-autojs6/
  ```
- AutoJs6 无障碍服务之前已启用。
- `probe.js` 曾运行到 Android 16 MediaProjection 截图授权页，用户已手动选择“整个屏幕”。
- 已修复 AutoJs6 logger 的目录/日志文件创建问题，并同步到手机；尚未完成截图授权后的完整 probe 运行验证。
- 按用户要求，后续不再直接打开或查看截图。

## 项目已有实现

### Python 参考实现

保留：

```text
scripts/checkin.py
scripts/probe.py
scripts/analyze_images.py
scripts/test_image_states.py
```

Python 版本已经完成四态识别离线回归：

```text
gray
locating
ready
success
```

### AutoJs6 实现

目录：

```text
autojs6/
```

包含：

- `main.js`
- `probe.js`
- `config.js`
- `lib/device.js`
- `lib/vision.js`
- `lib/navigation.js`
- `lib/ocr.js`
- `lib/state_machine.js`
- `lib/logger.js`
- `lib/pcapdroid.js`
- `profiles/PHY110.json`
- 四态 fixture

默认配置仍然是：

```javascript
runtime.dryRun = true
pcapdroid.enabled = false
```

## 下一步恢复顺序

暂停结束后，不要直接查看图片，按以下顺序继续：

1. 先确认 SDK 状态和是否有残留下载：
   ```powershell
   $sdk='D:\Documents\Android\Sdk'
   Get-ChildItem "$sdk\system-images" -Recurse -Depth 3
   ```
2. 用 Android Studio 自带 JBR 设置 `JAVA_HOME`。
3. 重新安装一个 system image：
   - 优先继续 Google Play API 36 x86_64；
   - 如果下载过慢，改用体积更小的 `system-images/android-36/google_apis/x86_64` 做页面/定位测试。
4. 用 `avdmanager` 创建 AVD；推荐名称：`ZHFD_API36`。
5. 启动 AVD，等待：
   ```text
   sys.boot_completed=1
   ```
6. 用 SDK 自带 ADB 验证模拟器序列号通常为：
   ```text
   emulator-5554
   ```
7. 获取或提供合法的智汇福大 APK；不要复制实体机 Cookie/Token。
8. 只做安装、登录、定位和页面 dry-run 测试。
9. 用 UI hierarchy、logcat、包名、定位状态和脚本 JSON 判断结果，不查看截图。

## 相关文档

- `docs/EMULATOR_ENVIRONMENT.md`：电脑模拟器环境配置说明
- `docs/AUTOJS6_PCAPDROID_PLAN.md`：手机端 AutoJs6 + PCAPdroid 方案
- `docs/WORKLOG_2026-09-18.md`：项目历史工作日志
- `docs/CONTEXT.md`：总上下文
