# 2026-09-22 签到窗口内模拟器验证记录

> 窗口 21:30–23:59。全部为 dry-run，未点击签到。
> 证据存档：`captures/emulator/window_test_0922/`

## 结论速览

| 环节 | 结果 |
|---|---|
| 窗口内状态识别 | ✅ locating 蓝色识别准确（H≈108 S≈208 白字≈4300，与标定一致） |
| 定位注入 → 系统 | ✅ gps_hal 每次 H5 轮询（10s 周期）都成功回调注入坐标，`dumpsys location` 显示 `delivered location to cn.edu.fzu.fdxypa` |
| 定位注入 → App 原生 | ✅ App 原生层每次注册 GPS（HIGH_ACCURACY）后立即收到位置并反注册 |
| 定位 → H5 页面 | ❌ H5 一直卡在"定位中..."，未进入"点击签到" |
| 窗口结束行为 | ✅ 00:00 刚过页面立即从 locating 转 gray，说明页面与服务器时序同步正常 |
| 21:32 定时唤醒 | ❌ 任务被消费但未真正执行（当时会话不可用或被丢弃），需在窗口开启前手动保证会话空闲 |

## 状态时间线（当晚关键点）

```text
22:14  进签到页即 locating（蓝色"定位中..."，白字≈4351）
22:14-22:38  locating 持续，偶发 unknown→gray→locating 抖动
22:38  ldconsole locate 改坐标 26.064,119.193 后仍 locating
23:42  重进页面先 gray，一轮轮询后转 locating
00:00  窗口关闭，locating → gray（约 45 秒内完成）
```

## 已排除的原因

1. **权限问题**：ACCESS_FINE/COARSE 已授予（`pm grant`），`dumpsys package` 确认 granted=true。
2. **系统定位开关**：`location_mode=3`（高精度）。
3. **GPS 交付**：`dumpsys location` 日志区多次出现
   `gps provider delivered location[1] to 10071/cn.edu.fzu.fdxypa`，
   每次 App 注册后 <50ms 内就收到 fix。
4. **网络**：模拟器 WiFi 已验证连通（ping 223.5.5.5 通）。
5. **窗口/服务器时间**：窗口内窗口外状态切换准时（00:00 转 gray），
   说明 H5 与服务器通信正常。

## 未解决：H5 卡"定位中..."的疑点

现象：App 原生层收到坐标（每次都是"注册→立刻交付→反注册"的 900ms 短请求），
但 H5 页面长时间显示"定位中..."，从未变"点击签到"。

候选原因（按可能性排序）：

1. **坐标偏移/坐标系问题**：LD 引擎交付的坐标与 `ldconsole locate` 写入值存在
   ~1 km 偏移（22:14 配置 119.178 → 实际交付 119.1886）。若 App 做校区围栏判定
   （半径可能 <1km），偏移会落在围栏外 → 定位"成功"但判为不在校区 → 保持 locating。
2. **AMap SDK 模拟器兼容**：App 内置 `libAMapSDK_MAP_v9_6_2.so`，高德定位 SDK
   在部分模拟器上有已知兼容问题（返回错误码但不崩溃）。
3. **WGS84/GCJ-02 转换**：系统 GPS 给的是 WGS84，高德用 GCJ-02；若 App 直接把
   WGS84 坐标当 GCJ-02 用，位置会偏移 300-700m，可能掉出围栏。

## 下次验证建议（21:30 窗口前准备）

1. **把注入点精确放到校区围栏内多个位置逐一试**：
   - 学生街（App 成功样例显示的地址"上街镇学府北路10-1岐安福大学生街"）：
     高德坐标约 26.0628, 119.1952 附近，可从真机地图 App 取精确点；
   - 每次改坐标后 `am force-stop` App 重进签到页，观察 10 分钟。
2. **抓一次 H5 的实际反应**：`adb shell logcat -c` 后重进页面，
   立即 `logcat -d > file` 全量保存，找 flutter 标签里
   `nativeCallJsLocationFunKysk` 之后 App 回调给 JS 的内容（若有打印）。
3. **对照真机**：真机同时段进签到页记录从 locating→ready 的耗时
   （页面提示"受定位服务流量限制，预计等待时间约1-2分钟"——
   模拟器上等 20 分钟都没变，超过真机正常时长的 10 倍，基本排除"只是慢"）。
4. 若定位链路最终无法在模拟器走通，考虑改用真机验证（同一份脚本
   `ZHFD_SERIAL=<真机 serial>` 即可），模拟器保留作页面/回归测试台。

## 复现命令

```bash
# 状态监控（不点击）
.conda/python.exe -u -c "
import uiautomator2 as u2, sys, time
sys.path.insert(0, 'scripts')
import checkin as ck
d = u2.connect('127.0.0.1:5555')
t0=time.time(); last=None
while time.time()-t0 < 300:
    img = d.screenshot(format='opencv')
    box = ck.find_button_circle(img)
    state = ck.classify_button_image(ck.crop_image(img, box), verbose=False) if box else 'nobox'
    if state != last:
        print(time.strftime('%H:%M:%S'), state, flush=True); last = state
    if state in ('ready','success'): break
    time.sleep(6)"
```
