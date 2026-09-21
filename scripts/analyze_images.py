# -*- coding: utf-8 -*-
"""用 OpenCV 读取 data/images 中的签到截图。

本项目页面是 Flutter 自绘，hierarchy/OCR 不可靠；这个工具以截图视觉分析为主，
输出按钮颜色、白字像素数量和推断状态。若本机以后安装 OCR，可在此脚本上再接入 OCR。
"""
from pathlib import Path
import cv2
import numpy as np

ROOT = Path(__file__).resolve().parents[1]
IMAGE_DIR = ROOT / "data" / "images"
CENTER_RATIO = 1756 / 3168
BUTTON_RATIO = 420 / 1440
SIZE = 315
LABELS = {
    "IMG_20260918_14032320.jpeg": "启动白屏/校徽",
    "IMG_20260918_14033051.jpeg": "启动页/校训",
    "IMG_20260918_14033661.jpeg": "首页加载中",
    "IMG_20260918_14034031.jpeg": "首页我的服务（首次）",
    "IMG_20260918_14034738.jpeg": "首页我的服务（加载后）",
    "IMG_20260918_14040392.jpeg": "签到页加载中",
    "IMG_20260918_14040981.jpeg": "灰色：无法签到/未到时间",
    "IMG_20260919_15503298.jpeg": "蓝色：定位中...",
    "IMG_20260919_15503662.jpeg": "蓝色：点击签到",
    "IMG_20260919_15504045.jpeg": "绿色：签到成功（带提示）",
    "IMG_20260919_15504871.jpeg": "绿色：签到成功",
}


def button_crop(image):
    height, width = image.shape[:2]
    cx, cy = width // 2, int(round(height * CENTER_RATIO))
    size = int(round(width * BUTTON_RATIO))
    x1, x2 = cx - size // 2, cx + size // 2
    y1, y2 = cy - size // 2, cy + size // 2
    crop = image[max(0, y1):min(height, y2), max(0, x1):min(width, x2)]
    return cv2.resize(crop, (SIZE, SIZE), interpolation=cv2.INTER_AREA)


def inspect(image):
    hsv = cv2.cvtColor(image, cv2.COLOR_BGR2HSV)
    yy, xx = np.ogrid[:SIZE, :SIZE]
    circle = (xx - SIZE / 2) ** 2 + (yy - SIZE / 2) ** 2 < (SIZE * 0.43) ** 2
    pixels = hsv[circle].astype(np.float32)
    colored = pixels[(pixels[:, 1] > 40) & (pixels[:, 2] > 60)]
    h = float(np.median(colored[:, 0])) if len(colored) else 0.0
    s = float(np.median(pixels[:, 1]))
    v = float(np.median(pixels[:, 2]))
    coverage = len(colored) / max(1, len(pixels))
    white = int(((hsv[:, :, 1] < 80) & (hsv[:, :, 2] > 235) & circle).sum())
    if coverage > 0.35 and 35 <= h <= 85:
        state = "success"
    elif coverage > 0.55 and 95 <= h <= 130:
        state = "ready" if white >= 5000 else "locating"
    elif s <= 40 and v < 230:
        state = "gray"
    elif coverage == 0 and v >= 230:
        state = "loading/unknown"
    else:
        state = "unknown"
    return state, h, s, v, coverage, white


def main():
    for path in sorted(IMAGE_DIR.glob("*.jpeg")):
        image = cv2.imread(str(path))
        if image is None:
            continue
        state, h, s, v, coverage, white = inspect(button_crop(image))
        label = LABELS.get(path.name, "未标注")
        print(
            f"{path.name}: {label} | state={state} "
            f"H={h:.1f} Smed={s:.1f} Vmed={v:.1f} "
            f"color={coverage:.2f} white={white}"
        )


if __name__ == "__main__":
    main()
