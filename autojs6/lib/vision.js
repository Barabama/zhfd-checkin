var CFG = require("../config");

var STATE = {
    UNKNOWN: "unknown",
    GRAY: "gray",
    LOCATING: "locating",
    READY: "ready",
    SUCCESS: "success"
};

function clamp(v, lo, hi) { return Math.max(lo, Math.min(hi, v)); }

function buttonBox() {
    var w = Number(device.width), h = Number(device.height);
    var r = CFG.vision.buttonBoxRatio;
    var cx = Math.round(w * CFG.vision.buttonCenterRatio[0]);
    var cy = Math.round(h * CFG.vision.buttonCenterRatio[1]);
    return { cx: cx, cy: cy, w: Math.round(w * r[0]), h: Math.round(h * r[1]) };
}

function rgbToHsv(r, g, b) {
    r /= 255; g /= 255; b /= 255;
    var max = Math.max(r, g, b), min = Math.min(r, g, b), d = max - min;
    var h = 0;
    if (d !== 0) {
        if (max === r) h = ((g - b) / d) % 6;
        else if (max === g) h = (b - r) / d + 2;
        else h = (r - g) / d + 4;
        h *= 60;
        if (h < 0) h += 360;
    }
    return [h / 2, max === 0 ? 0 : d / max * 255, max * 255];
}

function median(values) {
    if (!values.length) return 0;
    values.sort(function (a, b) { return a - b; });
    return values[Math.floor(values.length / 2)];
}

function analyzeButton(image) {
    var box = buttonBox();
    var radius = Math.round(Math.min(box.w, box.h) * 0.43);
    var step = Math.max(3, Math.round(Math.min(box.w, box.h) / 100));
    var hs = [], ss = [], vs = [], colored = 0, white = 0, total = 0;
    var dominantH = [];
    for (var y = -radius; y <= radius; y += step) {
        for (var x = -radius; x <= radius; x += step) {
            if (x * x + y * y > radius * radius) continue;
            var px = images.pixel(image, clamp(box.cx + x, 0, device.width - 1), clamp(box.cy + y, 0, device.height - 1));
            var hsv = rgbToHsv(colors.red(px), colors.green(px), colors.blue(px));
            hs.push(hsv[0]); ss.push(hsv[1]); vs.push(hsv[2]); total++;
            if (hsv[1] > 40 && hsv[2] > 60) { colored++; dominantH.push(hsv[0]); }
            if (hsv[1] < 80 && hsv[2] > 235) white++;
        }
    }
    var h = median(hs), s = median(ss), v = median(vs);
    var dh = dominantH.length ? median(dominantH) : 0;
    var coverage = colored / Math.max(1, total);
    var whiteRatio = white / Math.max(1, total);
    var gray = CFG.vision.gray;
    var blue = CFG.vision.blue;
    var green = CFG.vision.green;
    var state = STATE.UNKNOWN;
    // A nearly white or black frame is a loading/locked screen, never a gray sign-in state.
    if (coverage < 0.05 && (v >= 235 || v < 80)) {
        state = STATE.UNKNOWN;
    } else if (coverage > green.minCoverage && dh >= green.minH && dh <= green.maxH) {
        state = STATE.SUCCESS;
    } else if (coverage > blue.minCoverage && dh >= blue.minH && dh <= blue.maxH) {
        state = whiteRatio >= CFG.vision.readyWhiteRatio ? STATE.READY : STATE.LOCATING;
    } else if (s <= gray.maxS && v >= gray.minV && v < gray.maxV) {
        state = STATE.GRAY;
    }
    return { state: state, h: h, s: s, v: v, dominantH: dh,
        coverage: coverage, whiteRatio: whiteRatio, box: box, samples: total };
}

function pageState(image) {
    return analyzeButton(image);
}

function isCheckinState(state) {
    return state === STATE.GRAY || state === STATE.LOCATING ||
        state === STATE.READY || state === STATE.SUCCESS;
}

function isGreenPixel(hsv) {
    return hsv[1] >= 80 && hsv[2] >= 80 && hsv[0] >= 35 && hsv[0] <= 95;
}

function findGreenServiceIcon(image) {
    var w = device.width, h = device.height;
    var y1 = Math.round(h * CFG.vision.greenIconRegionRatio[0]);
    var y2 = Math.round(h * CFG.vision.greenIconRegionRatio[1]);
    var step = Math.max(6, Math.round(w / 180));
    var gw = Math.ceil(w / step), gh = Math.ceil((y2 - y1) / step);
    var grid = [], points = [];
    for (var gy = 0; gy < gh; gy++) {
        grid[gy] = [];
        for (var gx = 0; gx < gw; gx++) {
            var x = Math.min(w - 1, gx * step + Math.floor(step / 2));
            var y = Math.min(h - 1, y1 + gy * step + Math.floor(step / 2));
            var px = images.pixel(image, x, y);
            var hsv = rgbToHsv(colors.red(px), colors.green(px), colors.blue(px));
            var ok = isGreenPixel(hsv);
            grid[gy][gx] = ok;
            if (ok) points.push({ x: x, y: y });
        }
    }
    // 简化连通域：按网格 BFS，选择服务区内近似方形的大绿色块。
    var seen = [], candidates = [];
    for (var a = 0; a < gh; a++) seen[a] = [];
    for (var sy = 0; sy < gh; sy++) for (var sx = 0; sx < gw; sx++) {
        if (!grid[sy][sx] || seen[sy][sx]) continue;
        var q = [{x: sx, y: sy}], n = 0, minX = sx, maxX = sx, minY = sy, maxY = sy;
        seen[sy][sx] = true;
        while (q.length) {
            var p = q.shift(); n++;
            minX = Math.min(minX, p.x); maxX = Math.max(maxX, p.x);
            minY = Math.min(minY, p.y); maxY = Math.max(maxY, p.y);
            [[1,0],[-1,0],[0,1],[0,-1]].forEach(function (d) {
                var nx = p.x + d[0], ny = p.y + d[1];
                if (nx >= 0 && nx < gw && ny >= 0 && ny < gh && grid[ny][nx] && !seen[ny][nx]) {
                    seen[ny][nx] = true; q.push({x: nx, y: ny});
                }
            });
        }
        var bw = (maxX - minX + 1) * step, bh = (maxY - minY + 1) * step;
        if (n >= 40 && bw >= w * 0.04 && bh >= h * 0.025 && bw / bh >= 0.55 && bw / bh <= 1.8) {
            candidates.push({ x: (minX + maxX) * step / 2, y: y1 + (minY + maxY) * step / 2,
                w: bw, h: bh, area: n });
        }
    }
    if (!candidates.length) return null;
    candidates.sort(function (a, b) { return a.y - b.y || a.x - b.x; });
    return { x: Math.round(candidates[0].x), y: Math.round(candidates[0].y),
        bounds: candidates[0] };
}

module.exports = { STATE: STATE, buttonBox: buttonBox, analyzeButton: analyzeButton,
    pageState: pageState, isCheckinState: isCheckinState, findGreenServiceIcon: findGreenServiceIcon };
