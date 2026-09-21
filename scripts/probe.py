# -*- coding: utf-8 -*-
"""智汇福大签到页面探测工具。

常用流程：
  1. 手机授权后，手动打开“智汇福大”；
  2. 运行：python scripts/probe.py --navigate
  3. 脚本会读取当前包名，尝试点击“我的服务”->“晚点名签到”，并保存截图/层级树；
  4. 若按钮是自绘/H5，再用 --sample cx,cy,w,h 采集真实颜色。
"""
import os
import re
import sys
import time
from pathlib import Path

ADB_DIR = Path(os.getenv("ANDROID_ADB_DIR", r"D:\Programs\platform-tools"))
if ADB_DIR.is_dir():
    os.environ["PATH"] = str(ADB_DIR) + os.pathsep + os.environ.get("PATH", "")

import cv2
import numpy as np
import uiautomator2 as u2

# 探测输出统一保存到项目根的 debug/ 目录。
DEBUG_DIR = Path(__file__).resolve().parents[1] / "debug"
DEBUG_DIR.mkdir(exist_ok=True)

APP_LABEL = "智汇福大"
SERVICE_TAB = "我的服务"
CHECKIN_ENTRY = "晚点名签到"
PKG = os.getenv("ZHFD_PKG", "")


def connect_device():
    serial = os.getenv("ANDROID_SERIAL") or os.getenv("ZHFD_SERIAL")
    try:
        return u2.connect(serial) if serial else u2.connect()
    except Exception as exc:
        print(f"[连接失败] {exc}")
        print(r"请在手机上允许 USB 调试，并确认 D:\Programs\platform-tools\adb.exe devices 显示 device（不是 unauthorized）。")
        sys.exit(2)


def click_text(d, text, timeout=3):
    obj = d(text=text)
    if obj.wait(timeout=timeout):
        info = obj.info
        print(f"[点击] {text!r} bounds={info.get('bounds')}")
        obj.click()
        time.sleep(1)
        return True
    return False


def navigate_to_checkin(d):
    """按已知视觉路径进入：我的服务 -> 晚点名签到。"""
    d.screenshot(str(DEBUG_DIR / "debug_home.png"))
    if click_text(d, SERVICE_TAB):
        d.screenshot(str(DEBUG_DIR / "debug_my_services.png"))
    if click_text(d, CHECKIN_ENTRY):
        d.screenshot(str(DEBUG_DIR / "debug_checkin_page.png"))
        return True
    print(f"[未找到] {CHECKIN_ENTRY!r}；可能是 H5/自绘页面，改用截图和模板定位。")
    return False


def print_text_hits(xml):
    for kw in ("我的服务", "晚点名签到", "签到", "打卡"):
        hits = set(re.findall(r'text="([^"]*%s[^"]*)"' % re.escape(kw), xml))
        if hits:
            print(f"[原生可读] {kw}: {sorted(hits)}")


def save_page_dump(d, stem="page"):
    xml = d.dump_hierarchy()
    (DEBUG_DIR / f"{stem}_hierarchy.xml").write_text(xml, encoding="utf-8")
    d.screenshot(str(DEBUG_DIR / f"{stem}.png"))
    print_text_hits(xml)
    print(f"已保存 {DEBUG_DIR / (stem + '.png')} 和 {DEBUG_DIR / (stem + '_hierarchy.xml')}")


def sample_roi(d, box):
    cx, cy, w, h = box
    image = d.screenshot(format="opencv")
    roi = image[cy-h//2:cy+h//2, cx-w//2:cx+w//2]
    if roi.size == 0:
        raise ValueError("采样区域超出屏幕范围")
    hsv = cv2.cvtColor(roi, cv2.COLOR_BGR2HSV).reshape(-1, 3).astype(np.float32)
    h_values, s_values, v_values = hsv.T
    print(
        "H(mean/median)=%.1f/%.1f  S(mean/median/p90)=%.1f/%.1f/%.1f  V(mean/median)=%.1f/%.1f"
        % (
            h_values.mean(), np.median(h_values), s_values.mean(),
            np.median(s_values), np.percentile(s_values, 90),
            v_values.mean(), np.median(v_values),
        )
    )


def main():
    d = connect_device()
    current = d.app_current()
    print("前台应用:", current)
    print("设备屏幕:", d.window_size())

    if "--sample" in sys.argv:
        index = sys.argv.index("--sample") + 1
        if index >= len(sys.argv):
            print("用法: python scripts/probe.py --sample cx,cy,w,h", file=sys.stderr)
            sys.exit(2)
        try:
            box = tuple(map(int, sys.argv[index].split(",")))
            if len(box) != 4:
                raise ValueError
        except ValueError:
            print("坐标必须是 cx,cy,w,h，例如 540,1400,120,120", file=sys.stderr)
            sys.exit(2)
        sample_roi(d, box)
        return

    if PKG:
        print(f"启动目标包: {PKG}")
        d.app_start(PKG)
        time.sleep(4)
    else:
        print(f"未设置 ZHFD_PKG；保留当前前台应用（应为“{APP_LABEL}”）。")

    if "--navigate" in sys.argv:
        navigate_to_checkin(d)
        save_page_dump(d, "page")
        return

    try:
        input(">>> 请手动进入“我的服务”->“晚点名签到”，页面停稳后按回车...")
    except EOFError:
        print("[非交互模式] 检测到无标准输入，跳过手动导航等待，直接保存当前页面。")
    save_page_dump(d, "page")


if __name__ == "__main__":
    main()
