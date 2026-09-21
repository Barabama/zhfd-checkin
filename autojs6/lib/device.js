var CFG = require("../config");

function info() {
    return {
        model: String(device.model || "unknown"),
        brand: String(device.brand || "unknown"),
        sdk: String(device.sdkInt || "unknown"),
        width: Number(device.width),
        height: Number(device.height),
        rotation: Number(device.rotation || 0),
        screenOn: typeof device.isScreenOn === "function" ? device.isScreenOn() : true,
        packageName: typeof currentPackage === "function" ? currentPackage() : "unknown",
        activity: typeof currentActivity === "function" ? currentActivity() : "unknown"
    };
}

function wakeAndUnlock() {
    try {
        if (typeof device.wakeUpIfNeeded === "function") device.wakeUpIfNeeded();
        else if (typeof device.wakeUp === "function") device.wakeUp();
    } catch (e) {}
    sleep(600);
    try { if (typeof device.unlock === "function") device.unlock(); } catch (e2) {}
    sleep(600);
    // 当前 PHY110 无密码锁屏需要上滑；如果已在 App 内，避免再次滑动破坏页面。
    var pkg = typeof currentPackage === "function" ? currentPackage() : "";
    if (pkg === "com.android.systemui" || pkg === "com.android.launcher") {
        swipe(device.width * 0.5, device.height * 0.86,
            device.width * 0.5, device.height * 0.22, 350);
        sleep(800);
    }
}

function launchTarget() {
    app.launchPackage(CFG.app.packageName);
    waitForPackage(CFG.app.packageName, 10000);
    sleep(4000);
}

function requestCapture() {
    // 必须在一个主脚本中申请一次，然后将同一张 ImageWrapper 交给 OCR/视觉函数。
    // AutoJs6 新版本存在跨线程重复申请截图权限的问题，故不在子模块重复申请。
    var ok = false;
    try {
        ok = images.requestScreenCapture(false);
    } catch (e) {
        try { ok = requestScreenCapture(false); } catch (e2) { ok = false; }
    }
    return !!ok;
}

function capture() {
    return images.captureScreen();
}

function tapRatio(ratio) {
    click(Math.round(device.width * ratio[0]), Math.round(device.height * ratio[1]));
}

module.exports = {
    info: info,
    wakeAndUnlock: wakeAndUnlock,
    launchTarget: launchTarget,
    requestCapture: requestCapture,
    capture: capture,
    tapRatio: tapRatio
};
