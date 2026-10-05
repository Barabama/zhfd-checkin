# -*- coding: utf-8 -*-
"""智汇福大 v1.17.x 自动签到。

状态来自 2026-09-18/19 的真实截图（模拟器 2026-09-21 复验）：
- gray：灰色“无法签到”，未到时间；
- locating：蓝色“定位中...”，等待定位服务；
- ready：蓝色“点击签到”，允许点击；
- success：绿色“签到成功”，结束。

签到页是 App 内 WebView 加载的 H5（yzsxg.fzu.edu.cn），首页为 Flutter/自绘。
按钮位置随屏幕长宽比变化，因此用圆检测动态定位按钮，HSV 判态；
真机 1440x3168 与模拟器 900x1600 通用。
"""
import os
import time
from pathlib import Path

ADB_DIR = Path(os.getenv("ANDROID_ADB_DIR", r"D:\Programs\platform-tools"))
if ADB_DIR.is_dir():
    os.environ["PATH"] = str(ADB_DIR) + os.pathsep + os.environ.get("PATH", "")

import cv2
import numpy as np
import requests
import uiautomator2 as u2

# 调试截图统一输出到项目根的 debug/ 目录，避免污染运行目录。
DEBUG_DIR = Path(__file__).resolve().parents[1] / "debug"
DEBUG_DIR.mkdir(exist_ok=True)


def debug_shot(device, name):
    device.screenshot(str(DEBUG_DIR / name))


# ---------------- 智汇福大配置 ----------------
PKG = os.getenv("ZHFD_PKG", "cn.edu.fzu.fdxypa")
APP_NAME = "智汇福大"
SERVICE_TAB = "我的服务"
CHECKIN_ENTRY = "晚点名签到"
# 1440x3168 真机截图标定的签到圆按钮 (cx, cy, w, h)。
# 仅作动态检测失败时的回退：按屏幕比例换算。
BTN_BOX_FALLBACK_RATIO = (0.5, 0.673, 0.29, 0.29)  # 模拟器实测；真机为 0.5,0.5543
WINDOW = (
    os.getenv("ZHFD_WINDOW_START", "21:00"),
    os.getenv("ZHFD_WINDOW_END", "23:59"),
)
BARK_URL = os.getenv("BARK_URL", "")
DRY_RUN = os.getenv("ZHFD_DRY_RUN", "0") == "1"
# 真实截图的主色：蓝色 H 约 104/108，绿色 H 约 61，灰色 S=0。
GREEN = dict(h=(35, 85), s=(60, 255))
GRAY = dict(s=(0, 40))
BLUE = dict(h=(95, 130), s=(80, 255))
NORMALIZED_BUTTON_SIZE = 315
# ----------------------------------------------------


STATE_GRAY = "gray"
STATE_LOCATING = "locating"
STATE_READY = "ready"
STATE_SUCCESS = "success"
STATE_UNKNOWN = "unknown"


def notify(msg):
    print(time.strftime("[%H:%M:%S]"), msg)
    if BARK_URL:
        try:
            requests.get(f"{BARK_URL}/{msg}", timeout=5)
        except Exception:
            pass


def connect_device():
    serial = os.getenv("ANDROID_SERIAL") or os.getenv("ZHFD_SERIAL")
    try:
        return u2.connect(serial) if serial else u2.connect()
    except Exception as exc:
        notify(f"✗ 无法连接 Android 设备: {exc}")
        print(r"请确认 adb devices 显示 device（不是 unauthorized），并开启 USB 调试。")
        return None


def in_window():
    return WINDOW[0] <= time.strftime("%H:%M") <= WINDOW[1]


def click_text(d, text, timeout=3):
    obj = d(text=text)
    if obj.wait(timeout=timeout):
        print(f"[点击] {text!r} bounds={obj.info.get('bounds')}")
        obj.click()
        time.sleep(1)
        return True
    return False


def btn_box_from_ratio(device):
    """按屏幕比例换算回退按钮区域。"""
    w, h = device.window_size()
    cx, cy, wr, hr = BTN_BOX_FALLBACK_RATIO
    return (int(cx * w), int(cy * h), int(wr * w), int(hr * h))


def find_button_circle(image):
    """在签到页截图中用霍夫圆检测定位圆形签到按钮。

    按钮圆心 y 位置随屏幕长宽比/页面布局变化（真机 0.5543、模拟器 0.673），
    固定比例不可靠，改为每轮检测。检测窗口限定页面下半部，避免误检图标。
    返回 (cx, cy, w, h)，失败返回 None。
    """
    height, width = image.shape[:2]
    gray = cv2.cvtColor(image, cv2.COLOR_BGR2GRAY)
    gray = cv2.medianBlur(gray, 5)
    roi_y0 = int(height * 0.45)
    roi = gray[roi_y0 : int(height * 0.85)]
    r_min, r_max = int(width * 0.10), int(width * 0.28)
    circles = cv2.HoughCircles(
        roi,
        cv2.HOUGH_GRADIENT,
        dp=1.2,
        minDist=200,
        param1=120,
        param2=55,
        minRadius=r_min,
        maxRadius=r_max,
    )
    if circles is None:
        return None
    c = circles[0][0]
    cx, cy, r = int(c[0]), int(c[1]) + roi_y0, int(c[2])
    return (cx, cy, 2 * r, 2 * r)


def resolve_button_box(device):
    """优先动态圆检测，失败则用比例回退。"""
    box = find_button_circle(device.screenshot(format="opencv"))
    if box:
        print(f"[按钮] 圆检测: {box}")
        return box
    box = btn_box_from_ratio(device)
    print(f"[按钮] 回退比例区域: {box}")
    return box


def find_service_icon(d):
    """在首页“我的服务”区域找第一个绿色服务图标。

    模拟器上 hierarchy 的 content-desc 可读（“晚点名签到”），
    优先直接点描述节点；真机 Flutter 自绘读不到时降级为绿色视觉定位。
    """
    obj = d(description=CHECKIN_ENTRY)
    if obj.wait(timeout=3):
        bounds = obj.info.get("bounds", {})
        if all(k in bounds for k in ("left", "top", "right", "bottom")):
            pos = (
                (bounds["left"] + bounds["right"]) // 2,
                (bounds["top"] + bounds["bottom"]) // 2,
            )
            print(f"[入口] content-desc={CHECKIN_ENTRY!r} center={pos}")
            return pos
    image = d.screenshot(format="opencv")
    height, width = image.shape[:2]
    hsv = cv2.cvtColor(image, cv2.COLOR_BGR2HSV)
    mask = cv2.inRange(hsv, (35, 80, 80), (95, 255, 255))
    # 首页“我的服务”卡片区域约占屏幕 y=57%~78%（模拟器实测 0.657/0.771）。
    mask[:max(0, int(height * 0.57))] = 0
    mask[int(height * 0.80):] = 0
    contours, _ = cv2.findContours(mask, cv2.RETR_EXTERNAL, cv2.CHAIN_APPROX_SIMPLE)
    candidates = []
    for contour in contours:
        x, y, w, h = cv2.boundingRect(contour)
        if (
            width * 0.06 <= w <= width * 0.20
            and width * 0.06 <= h <= width * 0.20
            and 0.65 <= w / h <= 1.5
        ):
            candidates.append((y, x, w, h))
    if not candidates:
        return None
    y, x, w, h = min(candidates)
    pos = (x + w // 2, y + h // 2)
    print(f"[视觉入口] 绿色服务图标 bounds={(x, y, w, h)} center={pos}")
    return pos


def crop_image(image, box):
    cx, cy, w, h = map(int, box)
    height, width = image.shape[:2]
    x1, x2 = max(0, cx - w // 2), min(width, cx + w // 2)
    y1, y2 = max(0, cy - h // 2), min(height, cy + h // 2)
    return image[y1:y2, x1:x2]


def crop_roi(d, box):
    return crop_image(d.screenshot(format="opencv"), box)


def normalize_button(roi):
    return cv2.resize(
        roi,
        (NORMALIZED_BUTTON_SIZE, NORMALIZED_BUTTON_SIZE),
        interpolation=cv2.INTER_AREA,
    )


def classify_button_image(roi, verbose=True):
    """只根据按钮图像判断状态，不依赖 OCR。

    真实截图中：
    - 灰色按钮核心 V 中位数约 184，S=0；
    - 蓝色“定位中...”白字像素约 3.8k；
    - 蓝色“点击签到”包含时间，白字像素约 6.8k；
    - 绿色成功按钮 H 约 61。
    """
    if roi is None or roi.size == 0:
        return STATE_UNKNOWN
    image = normalize_button(roi)
    hsv = cv2.cvtColor(image, cv2.COLOR_BGR2HSV)
    yy, xx = np.ogrid[:NORMALIZED_BUTTON_SIZE, :NORMALIZED_BUTTON_SIZE]
    radius = NORMALIZED_BUTTON_SIZE * 0.43
    circle = (xx - NORMALIZED_BUTTON_SIZE / 2) ** 2 + (yy - NORMALIZED_BUTTON_SIZE / 2) ** 2 < radius**2
    pixels = hsv[circle].astype(np.float32)
    median_h, median_s, median_v = np.median(pixels, axis=0)
    colored = pixels[(pixels[:, 1] > 40) & (pixels[:, 2] > 60)]
    color_coverage = len(colored) / max(1, len(pixels))
    dominant_h = float(np.median(colored[:, 0])) if len(colored) else 0.0
    dominant_s = float(np.median(colored[:, 1])) if len(colored) else float(median_s)

    if verbose:
        print(
            "[按钮HSV] Hmed=%.1f Smed=%.1f Vmed=%.1f Hdom=%.1f Sdom=%.1f color=%.2f"
            % (median_h, median_s, median_v, dominant_h, dominant_s, color_coverage)
        )

    if color_coverage > 0.35 and GREEN["h"][0] <= dominant_h <= GREEN["h"][1]:
        return STATE_SUCCESS

    if color_coverage > 0.55 and BLUE["h"][0] <= dominant_h <= BLUE["h"][1]:
        # 仅统计圆心内的白字，避免把外圈和页面背景算进去。
        white_text = (hsv[:, :, 1] < 80) & (hsv[:, :, 2] > 235) & circle
        # 行结构判据（跨分辨率稳定）：
        # ready = "点击签到"上行(~y97-153) + 时间下行(~y180-213) 两段式；
        # locating = "定位中..."单行(~y130-180)。
        # 绝对像素数不可靠：模拟器 900x1600 渲染像素少（ready 实测 4345），
        # 真机 1080x2376 标定值是 locating≈3.7k / ready≈6.7k。
        proj = white_text.sum(axis=1)
        rows = []
        for y, v in enumerate(proj):
            if v > 3:
                if rows and y - rows[-1][1] <= 4:
                    rows[-1][1] = y
                    rows[-1][2] += int(v)
                else:
                    rows.append([y, y, int(v)])
        rows = [r for r in rows if r[2] >= 150]  # 过滤噪点行
        if verbose:
            print(f"[按钮文字行] {rows}")
        if len(rows) >= 2:
            # 两段式：上行(点击签到) + 下行(时间)。下行位置在圆下半部。
            lower_rows = [r for r in rows if r[0] >= NORMALIZED_BUTTON_SIZE * 0.5]
            if lower_rows:
                return STATE_READY
        # 单行宽文字 = 定位中...
        return STATE_LOCATING

    # 灰色按钮核心基本无彩色，且中位亮度明显低于纯白加载页。
    if 80 <= median_v < 230 and median_s <= GRAY["s"][1]:
        return STATE_GRAY
    return STATE_UNKNOWN


def classify(d, box, verbose=True):
    return classify_button_image(crop_roi(d, box), verbose=verbose)


def looks_like_checkin_page(d):
    """处理 App 保留上次路由、启动后已经停在签到页的情况。"""
    image = d.screenshot(format="opencv")
    box = find_button_circle(image)
    if box is None:
        return False
    state = classify_button_image(crop_image(image, box), verbose=False)
    if state != STATE_UNKNOWN:
        print(f"[页面] 当前已经是签到页，状态={state}")
        return True
    return False


def open_checkin_page(d):
    """进入“我的服务”->“晚点名签到”，兼容已在签到页的情况。"""
    if looks_like_checkin_page(d):
        debug_shot(d, "debug_checkin_page.png")
        return True
    debug_shot(d, "debug_home.png")
    if click_text(d, CHECKIN_ENTRY):
        debug_shot(d, "debug_checkin_page.png")
        return True
    pos = find_service_icon(d)
    if pos:
        d.click(*pos)
        time.sleep(3)
        debug_shot(d, "debug_checkin_page.png")
        return True
    return False


def native_button_box(d):
    """动态定位签到按钮：圆检测优先，hierarchy bounds 次之，比例回退兜底。"""
    for text in ("立即签到", "开始签到", "签到", "打卡"):
        obj = d(text=text)
        if obj.exists:
            bounds = obj.info.get("bounds", {})
            if all(k in bounds for k in ("left", "top", "right", "bottom")):
                box = (
                    (bounds["left"] + bounds["right"]) // 2,
                    (bounds["top"] + bounds["bottom"]) // 2,
                    max(1, bounds["right"] - bounds["left"]),
                    max(1, bounds["bottom"] - bounds["top"]),
                )
                print(f"[按钮] 原生控件 {text!r}: {box}")
                return box
    return resolve_button_box(d)


def wait_stable(d, box, timeout=15):
    prev, started = None, time.time()
    while time.time() - started < timeout:
        roi = crop_roi(d, box)
        if prev is not None and roi.shape == prev.shape:
            diff = np.abs(roi.astype(int) - prev.astype(int)).mean()
            if diff < 1.5:
                return True
        prev = roi
        time.sleep(1)
    return False


def wait_after_click(d, box, timeout=45):
    started = time.time()
    while time.time() - started < timeout:
        state = classify(d, box)
        debug_shot(d, "debug_state.png")
        if state == STATE_SUCCESS:
            return True
        if state == STATE_LOCATING:
            time.sleep(5)
            continue
        if state == STATE_READY:
            # 点击未生效时短暂重试一次，由调用方决定是否报告失败。
            time.sleep(2)
            continue
        time.sleep(2)
    return False


def ensure_unlocked(device):
    """唤醒并处理无密码锁屏；有密码时保留在锁屏并让主流程自然失败。"""
    if not device.info.get("screenOn"):
        device.screen_on()
        time.sleep(0.6)
    try:
        device.unlock()
    except Exception as exc:
        print(f"[解锁提示] {exc}")
    time.sleep(0.6)
    # OPPO/ColorOS 无密码锁屏通常还需要上滑一次，uiautomator2.unlock() 不一定会做。
    if device.info.get("currentPackageName") != PKG:
        device.swipe(0.5, 0.86, 0.5, 0.22, 0.35)
        time.sleep(0.8)


def main():
    device = connect_device()
    if device is None:
        return
    ensure_unlocked(device)
    device.app_start(PKG)
    time.sleep(4)
    if not open_checkin_page(device):
        return notify("✗ 未找到“我的服务”中的“晚点名签到”入口")

    box = native_button_box(device)
    stable = wait_stable(device, box)
    print(f"[页面稳定] {stable}, BTN_BOX={box}, WINDOW={WINDOW}, DRY_RUN={DRY_RUN}")
    debug_shot(device, "debug_loaded.png")

    while True:
        state = classify(device, box)
        debug_shot(device, "debug_state.png")

        if state == STATE_SUCCESS:
            return notify("✓ 今日已签到")
        if not in_window():
            return notify(
                f"当前不在签到窗口 {WINDOW[0]}-{WINDOW[1]}，页面状态={state}，未执行点击"
            )
        if state == STATE_READY:
            if DRY_RUN:
                return notify("[DRY_RUN] 检测到“点击签到”（ready），未点击")
            device.click(*box[:2])
            if wait_after_click(device, box):
                return notify("✓ 签到成功")
            return notify("? 点击后未在限定时间内看到“签到成功”，请查 debug_state.png")
        if state == STATE_LOCATING:
            print("[等待] 页面显示“定位中...”，15秒后重试")
            time.sleep(15)
            continue
        if state == STATE_GRAY:
            print("[等待] 页面显示“无法签到”，120秒后重试")
            time.sleep(120)
            continue
        return notify("? 未识别签到按钮状态，请查 debug_state.png")


if __name__ == "__main__":
    main()

