# 2026-09-23 会话收尾与恢复指南

> 2026-09-23 00:15 写。上一窗口（09-22 21:30–23:59）验证已完成并归档。
> 本文是"今晚到此"的收尾快照 + 下次会话的恢复入口。

## 项目当前一句话状态

模拟器（LDPlayer 14）的自动化链路（启动→导航→进签到页→四态识别→定位注入→系统交付 fix）
**全部打通**；唯一卡点是 **App 内 H5 页面在窗口内一直显示"定位中..."（locating），
从未转为"点击签到"（ready）**。已排除权限/网络/GPS 交付问题，
疑点集中在**坐标系偏移**与**高德定位 SDK 的模拟器兼容性**。

## 各文件当前角色

| 路径 | 说明 |
|---|---|
| `scripts/checkin.py` | 主脚本。四态状态机 + 霍夫圆动态按钮定位 + dry-run 边界。模拟器/真机通用 |
| `scripts/probe.py` | 页面探测（dump hierarchy + 截图 + HSV 采样） |
| `scripts/gcj2wgs.py` | GCJ-02 ↔ WGS-84 坐标互转。**注入 `ldconsole locate` 前必须用**（WGS-84） |
| `scripts/test_image_states.py` | 四态 fixture 离线回归（当前 PASS） |
| `scripts/analyze_images.py` | 截图 HSV 统计分析 |
| `docs/WINDOW_TEST_2026-09-22.md` | **09-22 窗口验证完整记录**（本轮最重要的文档） |
| `docs/LDPLAYER_ENVIRONMENT.md` | 模拟器环境 + ldconsole 用法 + 09-21/22 窗口验证记录 |
| `docs/CONTEXT.md` | 总上下文时间线 |
| `captures/emulator/window_test_0922/` | 窗口内证据：logcat ×4、dumpsys location ×5、页面截图 ×4 |

## 09-22 窗口验证结论（细节见 WINDOW_TEST 文档）

### 验证通过

1. 窗口内状态识别：`locating` 蓝色识别准确（H≈108 S≈208 白字≈4300，与真机标定一致）。
2. 定位注入端到端（到 App 原生层）：
   `ldconsole locate` → gps_hal `NewLocation` → `dumpsys location` 显示
   `delivered location to cn.edu.fzu.fdxypa`，每次 H5 轮询（10s 周期）都成功。
3. 窗口行为：21:30 窗口开启后 H5 才开始定位轮询；00:00 一过页面立即 locating→gray，
   与服务器时序同步正常。
4. 无人值守能力：脚本能在 App 冷启动掉回桌面后自动拉起（发现于 23:45 首跑失败）；
   但**脚本内 app_start 后等待 4 秒偏短**，模拟器冷启动需 ~12s，
   建议后续把 `checkin.py` 的等待改为轮询前台包名。

### 未解决：locating 不转 ready

- 注入坐标 `26.064,119.193`（WGS-84 校区东侧）与配置点偏移 ~1km（LD 引擎行为），
  App 高德 SDK 拿到坐标后 H5 仍不转"点击签到"。
- 三个候选原因：① LD 交付坐标偏移出校区围栏；② 高德定位 SDK 模拟器兼容问题；
  ③ WGS84/GCJ-02 混用（App 把 GPS 原始坐标当火星坐标用，偏 300-700m）。
- 页面提示"受定位服务流量限制，预计等待 1-2 分钟"（真机正常时长），
  模拟器上等待 20+ 分钟无变化，基本排除"只是慢"。

## 下次会话恢复步骤

> **09-23 23:27 更新**：恢复步骤 1-3 已在当夜半窗口（23:27-00:00）执行完毕：
> 学生街精确 WGS-84 坐标注入后仍卡 locating。结论与后续步骤见
> `docs/WINDOW_TEST_2026-09-23.md` —— 模拟器路线到顶，下一步转真机 USB 验证。

1. ~~读 `docs/WINDOW_TEST_2026-09-22.md`~~ → 已执行，另见 `WINDOW_TEST_2026-09-23.md`。
2. ~~坐标准备与注入~~ → 已执行（GCJ-02 26.0628,119.1952 → WGS-84 26.065882,119.190379）。
3. ~~窗口内监控~~ → 已执行（23:54:14 locating → 00:00:04 gray，未转 ready）。
4. ~~候选原因③（GCJ-02/WGS84 混用反推）~~ → 精确转换坐标也失败，该假设降级。
5. **新下一步：真机 USB 验证** —— 手机连电脑，
   `ZHFD_SERIAL=<真机serial> ZHFD_DRY_RUN=1 python scripts/checkin.py`。
6. **生产载体仍未定**（模拟器/真机 USB/手机 AutoJs6），真机验证后再评估。

## 定时唤醒的经验

09-21 挂的 21:32 一次性任务被消费但未实际执行（会话当时不可用）。
教训：**定时唤醒只在 Claude Code 会话开着且空闲时生效**；要无人值守就走
Windows 计划任务直跑 `scripts/checkin.py`（不依赖对话），待 ready 卡点解决后配置。

## 安全边界（不变）

dry-run（`ZHFD_DRY_RUN=1`）不点击签到；不伪造请求、不注入响应、不绕过定位/设备校验；
模拟器定位注入仅用于授权测试与页面行为验证。
