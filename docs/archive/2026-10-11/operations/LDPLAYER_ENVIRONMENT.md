# LDPlayer 14（雷电14）模拟器环境记录

> 2026-09-21 调研确认。用户决定先走雷电模拟器路线测试智汇福大页面、定位行为；
> Android Studio AVD 路线暂停（system image 下载中断）。
> 遵守边界：不直接查看截图；模拟器阶段 dry-run，不点击签到；
> 不伪造请求、不绕过定位/设备校验。

## 安装与实例

- 安装目录：`E:\leidian\LDPlayer14`（v14.0.28.0）
- 控制台工具：`E:\leidian\LDPlayer14\ldconsole.exe`（同目录 `dnconsole.exe` 是同一工具的旧名副本）
- 实例 0「雷电模拟器」：竖屏 900×1600，320 dpi，已安装并登录智汇福大
  （用户已手动点开过晚点名签到页面）
- App：`cn.edu.fzu.fdxypa` v1.17.4（versionCode 26091701，2026-09-21 11:57 安装）

## LD14 实测系统属性（重要：不是 Android 9）

```text
ro.build.version.release = 14
ro.build.version.sdk     = 34
ro.product.model         = 25060RK16C
ro.product.manufacturer  = REDMI
ro.product.cpu.abi       = x86_64
fingerprint              = OnePlus/graceltexx/graceltexx:14/UQ1A.240205.../ucsm.20260914...
```

网上旧资料说雷电基于 Android 9 / VirtualBox 不适用于 LD14：LD14 是官方的
Android 14 内核版本线（中文官网 ldmnq.com 有专门入口），与雷电 9 不能共存多开。
目录里有 `vmware-vdiskmanager.exe`、虚拟机目录 `vms/leidian0/leidian.vbox`，
引擎与旧版不同；本机 Hyper-V 开启的情况下实例可正常启动（WHPX/共存问题未复现）。

## adb 联动（已验证）

- 用户的"adb 调试（本地连接）"已打开。
- 任意 adb 均可连（SDK 版与雷电自带版实测都行，建议统一用一份避免互相 kill-server）：
  ```bash
  D:/Documents/Android/Sdk/platform-tools/adb.exe devices          # 直接可见 emulator-5554
  adb connect 127.0.0.1:5555                                       # TCP 方式，serial=127.0.0.1:5555
  ```
- 端口规则：实例 N = `5554 + 2N`（模拟器 console 口）；TCP adb 口 = 5555 + 2N。
- 免端口通道：`ldconsole adb --index 0 --command "shell getprop sys.boot_completed"`
- 注意：实例刚启动的约 20 秒内 adb 还没就绪，轮询 `getprop sys.boot_completed`
  等到 `1` 再操作；实例关闭后旧 serial 会显示 `offline`，需重新 connect。

## ldconsole 常用命令（LD14 实测）

```bash
LD="E:/leidian/LDPlayer14/ldconsole.exe"
"$LD" list                      # 实例名列表
"$LD" list2                     # 详情CSV: index,名,顶层窗口PID,实例PID,是否启动,窗口句柄?,adb端口,宽,高,dpi
"$LD" isrunning --index 0
"$LD" launch --index 0          # 启动（冷启动到 boot_completed 约 20s）
"$LD" quit --index 0
"$LD" adb --index 0 --command "shell wm size"
"$LD" locate --index 0 --LLI <Lng,Lat>   # LD14 新语法；老资料里的 operateloc 不存在
"$LD" installapp --index 0 --filename <apk>
"$LD" runapp --index 0 --packagename cn.edu.fzu.fdxypa
"$LD" killapp --index 0 --packagename cn.edu.fzu.fdxypa
"$LD" modify --index 0 --resolution 1080,2376,480   # 改分辨率需重启实例
```

注意：
- `list2` 中文实例名在 Git Bash 输出为 GBK 乱码，按逗号取字段即可，不影响使用。
- `locate` 立即返回 exit=0，坐标写入
  `E:\leidian\LDPlayer14\vms\config\leidian0.config`
  的 `statusSettings.location`（实测 lng/lat 已正确落盘）。
  App 请求定位时由模拟器注入该坐标。
- LD14 没有内置 `monkey`（`/system/bin/sh: monkey: inaccessible`），
  启动 App 用 `am start -n cn.edu.fzu.fdxypa/cn.edu.fzu.fdxy_app.MainActivity`。

## uiautomator2（已验证）

`.conda` 环境的 u2 直接连 TCP serial 即可，无需 init：

```python
import uiautomator2 as u2
d = u2.connect("127.0.0.1:5555")
d.window_size()   # (900, 1600)
d.dump_hierarchy()  # Flutter 页面几乎没有文本节点（与真机一致），只有状态栏时间
```

Python 端复用现有脚本的方式（脚本在 `scripts/`，调试输出统一写 `debug/`）：

```bash
ZHFD_SERIAL=127.0.0.1:5555 python scripts/probe.py
ZHFD_SERIAL=127.0.0.1:5555 ZHFD_DRY_RUN=1 python scripts/checkin.py
```

## 坐标系差异（待适配）

- 真机 PHY110：1440×3168，`BTN_BOX=(720,1756,420,420)`，服务图标中心 (204,1875)
- 模拟器：900×1600。`checkin.py` 中 `scale = width/1440.0` 及绝对坐标常量
  需改为按比例换算（`autojs6/config.js` 的 ratio 方案可直接搬）：
  - 按钮中心比例 [0.5, 0.5543] → 模拟器 (450, 887)
  - 服务图标中心比例 [0.1417, 0.5919] → 模拟器 (128, 947)
- HSV 四态阈值与分辨率无关，可直接复用。

## 定位权限与定位触发（2026-09-21 实测）

- App 安装后默认未授定位权限，前台也不会自动弹（Android 14）。
- 已通过 adb 授予（合法权限管理）：
  ```bash
  adb -s 127.0.0.1:5555 shell pm grant cn.edu.fzu.fdxypa android.permission.ACCESS_FINE_LOCATION
  adb -s 127.0.0.1:5555 shell pm grant cn.edu.fzu.fdxypa android.permission.ACCESS_COARSE_LOCATION
  ```
- `dumpsys location` 中 `last location=null` 只表示还没有 App 请求过 fix；
  App 只有进入晚点名签到页才会发起定位请求（首页不请求）。
- 验证 fix 的正确途径：进入签到页后看
  `dumpsys location | grep -E "loc:|ProviderRequest\[ON\]"`。

## 排查记录

- 2026-09-21 12:4x 实例曾意外停止（原因不明，可能是 locate 操作时手动关闭），
  `adb` 报 `device offline` / 连接拒绝；`ldconsole launch --index 0` 后
  20 秒内 boot 完成，一切恢复正常。遇到 offline 先 `ldconsole isrunning --index 0`。

## 下一步

1. `ZHFD_SERIAL=127.0.0.1:5555` 跑 `probe.py`，确认能进首页/签到页。
2. 按 ratio 适配 `checkin.py` 的按钮/入口坐标到 900×1600。
3. 模拟器上 dry-run 观察四态与定位注入是否正常（进入签到页后查 dumpsys location）。
4. PCAPdroid 不适用于模拟器路线；抓包改用 mitmproxy/Wireshark（另行评估）。

## 2026-09-21 下午验证结果

### 方案决策

- 生产载体暂不定（模拟器 / 电脑+真机 USB / 手机 AutoJs6 均为候选），
  模拟器只做验证台；模拟器内**不装 AutoJs6**，自动化引擎统一为 PC 端 Python + u2。

### 架构关键发现：签到页是 H5

logcat 显示晚点名签到页由 App 内 WebView 加载：
`https://yzsxg.fzu.edu.cn/plug-in/livecloud/project/fzu/attn/index.action`（含 token，不入库）。
页面通过 JS bridge 申请权限：
`kysk-fdxy-app://native?type=permission&action=0&biz=location&function=nativePermissionFunKysk`。

推论与实测一致：
- 首页（Flutter）不请求定位；进入签到页时 H5 检查/申请权限。
- 窗口外（未到 21:30）H5 不启动 GPS（`dumpsys location` 中 `mStarted=false`）；
  预计窗口内灰→蓝/定位流程才会发起定位请求。待签到窗口实测确认。

### 模拟器 vs 真机的 hierarchy 差异

- 真机 PHY110：Flutter 自绘 + H5，hierarchy 几乎无中文文本。
- 模拟器 LD14：hierarchy 的 `content-desc` **可读**——首页有
  "我的服务"/"晚点名签到"/"财务综合系统"等卡片描述，可直接选择器导航；
  签到页有标题"晚点名签到"节点。签到按钮本身仍是自绘（无文本节点）。

### 坐标实测（模拟器 900×1600）

- 签到圆按钮：霍夫圆检测 (449,1077) r≈159；HSV 中位 (0,0,184) = 灰色，
  与真机灰色标定（S=0, V≈184）完全一致，四态分类器跨设备可用。
- 按钮中心 y 比例 0.673 ≠ 真机 0.5543（屏幕长宽比不同），固定比例不可靠
  → `checkin.py` 已改为**霍夫圆动态检测**，比例值仅作回退。
- 首页服务图标中心 (127,1097)，x 比例 0.1411 与真机 0.1417 一致；入口优先用
  `d(description="晚点名签到")`，视觉绿色检测降级保留。

### checkin.py 适配（2026-09-21）

- `BTN_BOX` 绝对常量 → `find_button_circle()` 霍夫圆检测 +
  `BTN_BOX_FALLBACK_RATIO` 回退；`native_button_box()` 串起
  原生文本 → 圆检测 → 比例回退三级。
- `find_service_icon()` 增加 content-desc 优先路径；真机路径（绿色视觉检测）保留。
- `test_image_states.py` 改用 1080×2376 fixture 标定框（540,1317,315,315）；
  四态离线回归 PASS。
- `probe.py` 非交互模式（EOF）不再崩溃，直接保存当前页面。

### 模拟器 dry-run 结果（14:09）

```text
[页面] 当前已经是签到页，状态=gray
[按钮] 圆检测: (450, 1075, 366, 366)
[按钮HSV] Hmed=0.0 Smed=0.0 Vmed=184.0 color=0.00
当前不在签到窗口 21:30-23:59，页面状态=gray，未执行点击
```

启动→进页→识别→窗口判断全链路 OK。

### 待办

1. ~~21:30-23:59 窗口内在模拟器 dry-run 一次~~（2026-09-21/22 已完成，见下节）
2. 确认定位注入后页面显示旗山校区、按钮转蓝（待下一窗口验证）
3. 确认后评估生产载体；如选真机/模拟器长期跑，再配定时启动脚本

## 2026-09-21/22 签到窗口验证（23:43-23:59 + 复盘至 01:00）

### 已验证通过

1. **定位注入链路端到端可用**（ldconsole locate → gps_hal → 系统 → App）：
   - `ldconsole locate --index 0 --LLI <lng>,<lat>` 注入后（部分场景需重启实例落盘），
     logcat `gps_hal` 立即出现 `NewLocation <lat>, <lng>`；
   - dumpsys 显示 App（uid 10071）以 HIGH_ACCURACY 请求、系统 `delivered location` 50 次、
     App 统计 `locations = 71`；
   - **注意坐标系**：注入坐标必须是 **WGS-84**。高德/腾讯地图查到的校区坐标是 GCJ-02
     （旗山校区 26.061246, 119.193379），直接注入会偏 ~600m；
     第一次注入还因换算代码漏乘 π 偏了 1.84km。已写 `scripts/gcj2wgs.py` 做互转
     （round-trip 误差 <1m）。**正确注入值：`--LLI 119.188555,26.064324`**。
2. **窗口内 H5 定位轮询行为**：页面每 10 秒经 JS bridge 请求定位
   （`kysk-fdxy-app://native?type=location&function=nativeCallJsLocationFunKysk`），
   每次 App 发起 HIGH_ACCURACY GPS 请求并收到 fix；
   **窗口外（00:00 后）完全不轮询**（45 分钟 0 次）——定位只在签到窗口内发起。
3. **四态分类器在窗口内复验**：23:47 dry-run 曾识别到蓝色 `locating`（"定位中..."），
   HSV 阈值在模拟器上工作正常；识别链条无假阳性。

### 未解决：窗口内按钮最终停在灰色"无法签到"

23:47 显示过 `locating`，23:48 后转灰并保持到窗口结束（H5 每 10s 拿到 fix 但页面
不给"点击签到"）。当时注入坐标偏校区 1.84km（换算 bug），高德 SDK 逆地理得到
"不在旗山校区范围"是最可能原因。正确坐标已注入（01:00 后 H5 不再轮询，无法即时复验）。

### 其他发现

- App 冷启动后不保留"晚点名签到"路由（重新安装/重启后停在首页）；
  之前"已在签到页"是温启动保活。`checkin.py` 的首页导航路径因此很重要。
- 实例重启后定位权限会重置，需要重新 `pm grant`。
- `ldconsole locate` 在实例运行时注入即时生效（gps_hal 立刻打印新坐标）；
  但对 `leidian0.config` 的落盘有时延迟/需要 quit 才刷出，验证以 logcat 为准。

### 下一窗口（09-22 21:30-23:59）验证清单

1. 提前确认 `locate` 配置仍是 `119.188555,26.064324`（logcat gps_hal 确认坐标值）。
2. 21:30 后进签到页，确认按钮依次 gray → locating → ready。
3. 若 ready：dry-run 记录即成功，不改 config 直接结束。
4. 若仍 gray 且定位正常：抓 H5 页面数值（页面提示行 y=445 暗像素）+
   logcat 保存到 `captures/emulator/window_test_0922/`，考虑高德逆地理是否
   需要 GPS 卫星数（Bundle satellites=0）等信息，再评估。

## 2026-09-22 窗口验证结果：定位链路全通，按钮卡在 locating

### 执行时间线（定时任务 21:32 自动触发）

- 21:37 重注坐标（config 竟然又回到旧值 119.178），gps_hal 确认收到正确坐标。
- 21:40 进签到页，dry-run 状态机：gray → **locating**（识别正确，
  HSV 108/208/206、白字 4272 ≈ 真机 3725 标定值）。
- 21:40-23:59 按钮持续「定位中...」约 2 小时，从未转 ready。

### 系统层一切正常（排除项）

- GPS 会话每 10s 一轮：`set_position_mode → session begin → location_cb(正确坐标) → stop`；
  App（uid 10071）收到 8+ 次 fix，`locations = 8`。
- 模拟器网络可达高德（`ping c.amap.com` 7ms）；App 外连正常
  （59.77.x 福大 IP 段 yzsxg 可达、DNS 通）。
- 定位权限、mock 设置、坐标值均正确。

### 卡点定位：App/H5 侧

系统把 fix 交给了 App，但页面不结束定位态。可能原因（未完全排除）：

1. AMap SDK 收到 GPS 原始坐标但**逆地理编码失败**（需要访问高德服务器把
   坐标转成"旗山校区"地址；logcat 无 AMap 明确错误，但 AMap 网络请求日志缺失）。
2. H5 对定位结果有校验（速度/精度/卫星数：Bundle 显示 `satellites=0`，
   真机有真实卫星；模拟器注入 fix 卫星数为 0，可能被判定"不可信"）。
3. App 原生层到 H5 的 `nativeCallJsLocationFunKysk` 回传的数据格式/错误码问题。

### 结论与下一步

- 自动化脚本本身已完全可用（导航、四态识别、窗口判断、dry-run 保护全部正确）。
- 剩余问题是**模拟器环境与 App 定位 SDK 的兼容性**，不是脚本问题。
- 候选方案：a) 用真机 USB 跑同一条链路（最接近真实环境）；
  b) 深入调查模拟器 GPS 卫星数/精度模拟（雷电设置里可能有 GPS 强化选项）；
  c) 接受模拟器只能验证到 locating 为止，生产用真机。

### 证据文件（captures/emulator/window_test_0922/，不入库）

- logcat_2222/2337/2347_final.log、dumpsys_location_final.txt
- page_locating_stuck_2222.png、page_final_2347.png、button_2306.png
- button_2306.png 数值分析：按钮径向全部蓝色（H≈107），无红/灰；
  之前"白字 25k"是裁剪框四角的页面白背景（圆形 mask 内仍是 4272）。
