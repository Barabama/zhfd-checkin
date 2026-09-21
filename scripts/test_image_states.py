# -*- coding: utf-8 -*-
"""Offline regression for the Python reference classifier against real screenshots."""
from pathlib import Path
import sys
import cv2

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import checkin  # noqa: E402

CASES = {
    "gray": "autojs6/fixtures/gray.png",
    "locating": "autojs6/fixtures/locating.png",
    "ready": "autojs6/fixtures/ready.png",
    "success": "autojs6/fixtures/success.png",
}


def scaled_box(image):
    """Fixtures 是 1080x2376 真机截图，按钮中心 (540,1317)，区域 315x315。"""
    h, w = image.shape[:2]
    cx, cy = round(540 * w / 1080), round(1317 * h / 2376)
    size = round(315 * w / 1080)
    return (cx, cy, size, size)


def main():
    failures = []
    for expected, relative in CASES.items():
        path = ROOT / relative
        image = cv2.imread(str(path))
        if image is None:
            failures.append(f"missing: {relative}")
            continue
        actual = checkin.classify_button_image(checkin.crop_image(image, scaled_box(image)), verbose=False)
        print(f"{path.name}: expected={expected} actual={actual}")
        if actual != expected:
            failures.append(f"{path.name}: {actual} != {expected}")
    if failures:
        print("FAIL")
        for failure in failures:
            print(" -", failure)
        return 1
    print("PASS: all fixture states match")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
