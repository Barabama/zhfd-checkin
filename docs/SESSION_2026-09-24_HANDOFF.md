# 2026-09-24 项目进度总结与对接提示词

> 2026-09-24 22:18 写。供下一个 agent / 会话恢复使用。
> 恢复入口：先读本文，再按需读引用的文档。

## 一句话状态

模拟器（LDPlayer 14）路线已完成使命：自动化脚本全链路验证通过，
唯一遗留是**分类器把模拟器的 ready（点击签到）误判为 locating**（分辨率差异，已定位原因和修法）；
**下一步首选真机 USB 验证**，通过后定生产载体。

## 关键里程碑（按时间）

| 日期 | 事件 |
|---|---|
| 09-17/18 | 项目启动；Python+uiautomator2 方案；真机四态 HSV 标定（1080×2376 截图，按钮中心 540,1317） |
| 09-19 | 真机 dry-run 通过（唤醒→进页→识别 gray→不点击）；四态 fixture 回归 |
| 09-19/20 | AutoJs6 手机端骨架完成（`autojs6/`），卡在 MediaProjection 授权，后暂停 |
| 09-21 | 转向雷电模拟器 LD14；ldconsole/adb/u2 联动打通；dry-run 通过 |
| 09-21 | **架构发现**：签到页是 App 内 WebView 的 H5（yzsxg.fzu.edu.cn），JS bridge 申请定位 |
| 09-22 | 窗口内验证：定位注入全链路通，但 H5 卡"定位中..."不转 ready |
| 09-23 | 学生街精确 WGS-84 坐标注入仍卡 locating → **模拟器路线到顶** |
| 09-24 | **用户亲眼看到按钮转"点击签到"**；截图数值分析确认是 ready，但分类器误判为 locating |

## 09-24 重要转折：模拟器其实能转 ready！

此前两晚"一直卡 locating"的结论被推翻：09-24 22:09 用户观察到按钮
"定位中" → "点击签到"切换（定位耗时约 1-2 分钟，与真机一致）。
截图数值分析确认：

- 行结构两段式（上行"点击签到" + 下行时间行），与真机 ready fixture 逐像素吻合
- 但**白字像素总数 4345 < 分类器阈值 5000**（真机 ready 标定 6747），
  因为模拟器 900×1600 分辨率低，同样文字渲染像素少 → 被 5000 阈值误判为 locating

**已定位修复方案（未实施）**：分类器 `classify_button_image` 的 ready/locating
判据从"白字像素绝对数 ≥5000"改为**行结构判据**：
圆内白色文字出现"上行(约 y 97-153) + 下行(约 y 180-213)"两段 → ready；
单行(约 y 130-180) → locating。行位置跨分辨率稳定（模拟器/真机差 <6px）。

## 文件地图

| 路径 | 说明 |
|---|---|
| `scripts/checkin.py` | 主脚本：四态状态机 + 霍夫圆按钮定位 + dry-run。**需改 classify 判据** |
| `scripts/probe.py` | 页面探测（hierarchy/截图/HSV 采样） |
| `scripts/gcj2wgs.py` | GCJ-02↔WGS-84 互转。注入 `ldconsole locate` 前必须用它转 WGS-84 |
| `scripts/test_image_states.py` | 四态 fixture 离线回归（当前 PASS；改判据后必须仍 PASS） |
| `autojs6/` | 手机端 AutoJs6 骨架（暂停保留；若生产选手机独立运行再启用） |
| `docs/LDPLAYER_ENVIRONMENT.md` | 模拟器环境 + ldconsole 全部用法（launch/locate/adb 端口规则等） |
| `docs/WINDOW_TEST_2026-09-22.md` | 09-22 窗口验证（排除清单 + 复现命令） |
| `docs/WINDOW_TEST_2026-09-23.md` | 09-23 半窗口验证（坐标证伪记录） |
| `docs/CONTEXT.md` | 总上下文时间线（09-17 起） |
| `debug/button_now.png` | 09-24 的 ready 按钮截图（修分类器的对照样本） |
| `captures/emulator/window_test_0922|0923/` | 窗口内 logcat/dumpsys/截图证据 |

## 环境速查

```bash
# 模拟器
E:/leidian/LDPlayer14/ldconsole.exe isrunning --index 0   # 查状态
E:/leidian/LDPlayer14/ldconsole.exe launch --index 0       # 启动（boot 约 20-25s）
E:/leidian/LDPlayer14/ldconsole.exe locate --index 0 --LLI 119.190379,26.065882
# ↑ 学生街 WGS-84 坐标（GCJ-02 26.0628,119.1952 转换而来）；改坐标后 grep gps_hal 确认

# adb / u2
D:/Documents/Android/Sdk/platform-tools/adb.exe connect 127.0.0.1:5555
# serial = 127.0.0.1:5555（或 emulator-5554）；实例重启后需重授定位权限：
adb -s 127.0.0.1:5555 shell pm grant cn.edu.fzu.fdxypa android.permission.ACCESS_FINE_LOCATION
adb -s 127.0.0.1:5555 shell pm grant cn.edu.fzu.fdxypa android.permission.ACCESS_COARSE_LOCATION

# 真机
ADB: D:/Programs/platform-tools/adb.exe；序列号 402b184f（PHY110, Android 16, 1440×3168）

# 运行
cd d:/Documents/Projects/check-in-zhfd
.conda/python.exe scripts/test_image_states.py                          # 离线回归
ZHFD_SERIAL=127.0.0.1:5555 ZHFD_DRY_RUN=1 .conda/python.exe scripts/checkin.py
```

## 给下一个 agent 的对接提示词

> 复制以下内容作为新会话的第一条消息：

```text
继续 d:/Documents/Projects/check-in-zhfd 的智汇福大自动签到项目。
先读 docs/SESSION_2026-09-24_HANDOFF.md 了解全部进度，再按下面的优先级工作：

1. 修复 scripts/checkin.py 分类器的 ready/locating 判据：
   把"白字像素数 ≥5000"改为行结构判据（圆内白色文字两段式=ready，
   单行=locating），参考 handoff 文档"09-24 重要转折"一节的行位置数据；
   改完跑 scripts/test_image_states.py 必须仍 PASS，
   再用 debug/button_now.png（真 ready 样本）验证分类结果=ready。
2. 用模拟器做一次窗口内完整 dry-run（步骤见 LDPLAYER_ENVIRONMENT.md），
   确认状态机输出 gray→locating→ready 且 DRY_RUN 不点击。
3. 验证通过后与用户确认生产载体（真机 USB / 模拟器 / 手机 AutoJs6）；
   默认建议真机 USB：ZHFD_SERIAL=402b184f，脚本零改动。

安全边界（必须遵守）：
- 一切验证用 ZHFD_DRY_RUN=1，绝不点击"点击签到"按钮；
- 不伪造请求、不注入响应、不重放、不绕过定位/设备/时间校验；
- 用户要求：不直接查看截图做判断，用数值分析/JSON/hierarchy/logcat；
  （09-24 例外：读取了单张按钮裁剪图做视觉确认，属于用户许可的一次性确认）
- Token/Cookie/定位地址不写入日志和文档。
```

## 安全边界（长期有效）

- dry-run（`ZHFD_DRY_RUN=1`）不点击签到；只有用户明确说"关 dry-run"才允许真实点击，
  且首次真实点击需用户在场观察。
- 不伪造请求、不注入响应、不重放、不绕过定位/设备完整性校验。
- 模拟器定位注入仅用于授权测试与页面行为验证。
