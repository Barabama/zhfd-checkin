# 2026-09-24 真实点击签到验证：✅ 成功

> 23:44 窗口内（21:30-23:59）执行。用户在场并授权真实点击。
> 证据：captures/emulator/click_verify_0924/

## 结果

**雷电模拟器方案真实点击签到验证通过。** 全流程一次成功：

```text
[页面] 当前已经是签到页，状态=ready
[按钮] 圆检测: (449, 1075, 320, 320)
[页面稳定] True, BTN_BOX=(449, 1075, 320, 320), WINDOW=('21:30','23:59'), DRY_RUN=False
[按钮HSV] Hmed=108.0 Smed=208.0 Vmed=206.0 Hdom=108.0 color=0.94
[按钮文字行] [[107,151,3365], [179,204,903]]     ← 新行结构判据 → ready
[按钮HSV] Hmed=61.0 Smed=146.0 Vmed=164.0 Hdom=61.0 color=0.60   ← 点击后转绿
[23:44:01] ✓ 签到成功
```

点击后复检：`state: success`（HSV H=61 S=150 V=151，绿色覆盖率 0.94），
与真机 success 标定（H≈61）一致。

## 前置修复：分类器 ready/locating 判据

09-24 白天发现模拟器渲染的 ready 白字像素（4345）低于旧阈值（5000），
导致误判为 locating。已改为**行结构判据**（`scripts/checkin.py`）：

- 圆心内白色文字投影分行，过滤噪点行（<150px）；
- 两段式且存在下半部行（y ≥ 0.5×315）→ **ready**（"点击签到"上行 + 时间下行）；
- 单行宽文字 → **locating**（"定位中..."）。

回归验证：

- 四态 fixture 离线回归 **PASS**（gray/locating/ready/success）
- 09-24 真实 ready 样本 `debug/button_now.png` → **ready** ✅
- 模拟器实时页面 → **ready** ✅（修复前为 locating）

## 对"模拟器卡 locating"结论的修正

09-22/09-23 两晚记录的"H5 卡定位中不转 ready"结论**部分有误**：
09-24 实测定位约 1-2 分钟即转 ready（与真机提示一致），
此前判定"卡住"的原因至少有一部分是**分类器误判**（可见用户 09-24 目视确认）。
但 09-22/23 当时页面确实长时间显示定位中未变，两晚现象可能叠加了
分类器误判 + 定位耗时，具体比例无法回溯，应以 09-24 的实测为准。

## 复现命令（窗口内、用户在场时）

```bash
cd d:/Documents/Projects/check-in-zhfd
ZHFD_SERIAL=127.0.0.1:5555 .conda/python.exe scripts/checkin.py   # 不带 DRY_RUN=1 即真实点击
```

dry-run（安全模式）仍为：`ZHFD_DRY_RUN=1`。

## 安全边界提醒

真实点击仅在用户明确授权 + 在场时执行。默认仍是 dry-run；
`checkin.py` 只有显式 `ZHFD_DRY_RUN=0`（或未设置该变量）才点击。
