# 智汇福大 AutoJs6 手机端版本

## 当前状态

这是第一版手机端迁移骨架，默认 `dryRun=true`，不会点击“点击签到”。
现有 Python 版本仍是参考实现。

## 前置条件

1. 安装 AutoJs6（建议使用支持当前 Android 版本的发行版）。
2. 授予 AutoJs6 无障碍权限和截图权限。
3. 允许 AutoJs6 自启动、后台运行，并关闭电池优化限制。
4. 允许智汇福大使用定位。
5. 可选安装 PCAPdroid；首次先手动抓包，再启用自动控制。

AutoJs6 的截图权限只能由系统授权，脚本不会绕过授权；脚本在主入口只申请一次截图权限，再把同一张截图交给视觉/OCR模块，避免多线程重复申请问题。AutoJs6 的 OCR 接口在不同版本/插件中可能是 `ocr.paddle.detect` 或 `ocr.detect`，代码对此做了兼容回退。

## 导入和运行

当前 PHY110 已将脚本复制到 AutoJs6 默认工作路径：

```text
/storage/emulated/0/脚本/zhfd-autojs6/
```

在 AutoJs6 中进入：

```text
文件 → zhfd-autojs6
```

首次运行前，在 AutoJs6 侧拉抽屉中启用：

- 无障碍服务；
- 截图权限（运行 `probe.js` 时按系统提示允许）；
- 前台服务/后台运行；
- 忽略电池优化；
- 必要时允许“所有文件管理权限”。

将整个 `autojs6` 目录导入 AutoJs6，先运行：

```text
probe.js
```

探测脚本只截图、识别并写入 `logs/<timestamp>/probe.json`，不执行签到点击。
如果 AutoJs6 提示无法创建日志目录，请确认工作路径下已经存在 `logs` 文件夹；当前项目已预创建该目录。

随后运行：

```text
main.js
```

默认仍是 dry-run。确认真实窗口中能稳定识别“点击签到”后，才修改 `config.js`：

```javascript
runtime: {
    dryRun: false
}
```

## PCAPdroid

默认关闭：

```javascript
pcapdroid: {
    enabled: false
}
```

先手动在 PCAPdroid 中将 App Filter 设置为：

```text
cn.edu.fzu.fdxypa
```

确认能够正常捕获后，再改为 `enabled: true`。如果使用 PCAPdroid API Key，将它写入手机本地 `config.js`，不要提交到 Git 或日志。

脚本使用 PCAPdroid `CaptureCtrl` 的 `start` / `stop` Intent，输出到 `Download/PCAPdroid`。PCAPdroid 无法启动时，签到流程仍继续，但会在 `result.json` 记录错误。

## 当前设备参数

`profiles/PHY110.json`：

- 屏幕：1440×3168
- 签到按钮中心：(720, 1756)
- 首页“我的服务”第一项中心：(204, 1875)
- 签到时间：21:30—23:59

## 状态机

```text
gray      无法签到       等待 120 秒
locating  定位中...      等待 15 秒
ready     点击签到       dry-run 只记录；正式模式点击
success   签到成功       结束
```

## 安全边界

PCAPdroid 只用于观察连接、域名、时间、请求数量和 PCAP 时序；本项目不修改响应、不伪造请求、不重放签到请求，也不绕过定位、时间、身份或设备校验。

## PHY110 当前首次运行提示（2026-09-21）

已通过 AutoJs6 6.7.0 打开并运行 `probe.js`。当前流程已经到达 Android 16 的系统截图授权页，说明脚本已成功启动；尚未进入智汇福大，也没有执行签到点击。

请在手机上手动完成一次授权：

1. 在系统对话框点击“下一步”；
2. 如果出现录屏范围选择，选择“整个屏幕”（不要限制为 AutoJs6 单个应用，否则无法观察智汇福大）；
3. 点击“开始/立即开始/允许”等确认按钮；
4. 回到 AutoJs6，等待 `probe.js` 自动运行结束。

授权后可用以下方式确认结果，不需要查看截图：

```powershell
& 'D:\Programs\platform-tools\adb.exe' -s 402b184f shell 'find /storage/emulated/0/脚本/zhfd-autojs6/logs -maxdepth 3 -type f | sort'
& 'D:\Programs\platform-tools\adb.exe' -s 402b184f shell 'find /storage/emulated/0/脚本/zhfd-autojs6/logs -name probe.json -type f -exec cat {} \;'
```

后续调试优先读取 `probe.json`、`events.log`、AutoJs6 日志和当前前台包名，不直接查看图片。
