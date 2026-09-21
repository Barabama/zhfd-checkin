# -*- coding: utf-8 -*-
"""GCJ-02（火星坐标，高德/腾讯地图）与 WGS-84（GPS 原始坐标）互转。

模拟器 ldconsole locate 注入的是 WGS-84 坐标；App 内高德 SDK 按格雷
编码坐标（GCJ-02）计算校区范围。注入前必须把地图上查到的 GCJ-02
坐标转成 WGS-84，否则偏差数百米，可能导致 App 判定不在校区内。

用法:
  python scripts/gcj2wgs.py 26.061246 119.193379        # GCJ-02 -> WGS-84
  python scripts/gcj2wgs.py 26.064324 119.188555 --wgs  # WGS-84 -> GCJ-02
"""
import math
import sys

A = 6378245.0
EE = 0.00669342162296594323
PI = math.pi


def _tlat(x, y):
    r = -100.0 + 2.0 * x + 3.0 * y + 0.2 * y * y + 0.1 * x * y + 0.2 * math.sqrt(abs(x))
    r += (20.0 * math.sin(6.0 * x * PI) + 20.0 * math.sin(2.0 * x * PI)) * 2.0 / 3.0
    r += (20.0 * math.sin(y * PI) + 40.0 * math.sin(y / 3.0 * PI)) * 2.0 / 3.0
    r += (160.0 * math.sin(y / 12.0 * PI) + 320 * math.sin(y * PI / 30.0)) * 2.0 / 3.0
    return r


def _tlng(x, y):
    r = 300.0 + x + 2.0 * y + 0.1 * x * x + 0.1 * x * y + 0.1 * math.sqrt(abs(x))
    r += (20.0 * math.sin(6.0 * x * PI) + 20.0 * math.sin(2.0 * x * PI)) * 2.0 / 3.0
    r += (20.0 * math.sin(x * PI) + 40.0 * math.sin(x / 3.0 * PI)) * 2.0 / 3.0
    r += (150.0 * math.sin(x / 12.0 * PI) + 300.0 * math.sin(x / 30.0 * PI)) * 2.0 / 3.0
    return r


def _delta(lat, lng):
    dlat = _tlat(lng - 105.0, lat - 35.0)
    dlng = _tlng(lng - 105.0, lat - 35.0)
    magic = math.sin(lat / 180.0 * PI)
    magic = 1 - EE * magic * magic
    sq = math.sqrt(magic)
    dlat = (dlat * 180.0) / ((A * (1 - EE)) / (magic * sq) * PI)
    dlng = (dlng * 180.0) / (A / sq * math.cos(lat / 180.0 * PI) * PI)
    return dlat, dlng


def gcj2wgs(gcj_lat, gcj_lng):
    dlat, dlng = _delta(gcj_lat, gcj_lng)
    return gcj_lat - dlat, gcj_lng - dlng


def wgs2gcj(wgs_lat, wgs_lng):
    dlat, dlng = _delta(wgs_lat, wgs_lng)
    return wgs_lat + dlat, wgs_lng + dlng


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    if len(args) != 2:
        print(__doc__)
        return 2
    lat, lng = map(float, args)
    if "--wgs" in sys.argv:
        g = wgs2gcj(lat, lng)
        print("GCJ-02: lat=%.6f lng=%.6f" % g)
    else:
        w = gcj2wgs(lat, lng)
        print("WGS-84: lat=%.6f lng=%.6f" % w)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
