// OCR 是辅助定位；视觉状态识别不依赖 OCR。
function normalizeText(result) {
    if (!result) return [];
    var list = result instanceof Array ? result : (result.results || result.items || []);
    return list.map(function (item) {
        return {
            text: String(item.label || item.text || item.words || ""),
            bounds: item.bounds || item.rect || null,
            raw: item
        };
    }).filter(function (x) { return x.text.length > 0; });
}

function detect(image) {
    var result = null;
    try {
        if (typeof ocr !== "undefined" && ocr.paddle && ocr.paddle.detect) {
            result = ocr.paddle.detect(image);
        } else if (typeof ocr !== "undefined" && ocr.detect) {
            result = ocr.detect(image);
        }
    } catch (e) {
        console.warn("OCR unavailable: " + e);
    }
    return normalizeText(result);
}

function find(image, candidates) {
    var rows = detect(image);
    for (var i = 0; i < rows.length; i++) {
        for (var j = 0; j < candidates.length; j++) {
            if (rows[i].text.indexOf(candidates[j]) >= 0) return rows[i];
        }
    }
    return null;
}

function clickBounds(row) {
    if (!row || !row.bounds) return false;
    var b = row.bounds;
    var l = b.left !== undefined ? b.left : b.x;
    var t = b.top !== undefined ? b.top : b.y;
    var r = b.right !== undefined ? b.right : (b.x + b.width);
    var bo = b.bottom !== undefined ? b.bottom : (b.y + b.height);
    if ([l, t, r, bo].some(function (v) { return v === undefined || isNaN(v); })) return false;
    click(Math.round((l + r) / 2), Math.round((t + bo) / 2));
    return true;
}

module.exports = { detect: detect, find: find, clickBounds: clickBounds };
