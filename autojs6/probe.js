// 探测脚本：只截图和报告，不点击签到。
var CFG = require("./config");
var deviceLib = require("./lib/device");
var vision = require("./lib/vision");
var logger = require("./lib/logger");

var runDir = logger.createRunDir();
logger.log(runDir, "probe_start");
var info = deviceLib.info();
console.log(JSON.stringify(info, null, 2));
if (!deviceLib.requestCapture()) {
    logger.log(runDir, "screen_capture_denied");
    toast("请授予 AutoJs6 截图权限后重试");
    exit();
}
deviceLib.wakeAndUnlock();
app.launchPackage(CFG.app.packageName);
waitForPackage(CFG.app.packageName, 10000);
sleep(4000);
var image = deviceLib.capture();
var analysis = vision.analyzeButton(image);
logger.saveScreenshot(runDir, "probe", image);
var result = {
    model: info.model,
    android: info.sdk,
    screen: [device.width, device.height],
    package: currentPackage(),
    current_activity: currentActivity(),
    screen_on: device.isScreenOn ? device.isScreenOn() : true,
    suspected_page: vision.isCheckinState(analysis.state) ? "checkin" : "unknown",
    button: analysis,
    run_dir: runDir
};
logger.writeJson(files.join(runDir, "probe.json"), result);
console.log(JSON.stringify(result, null, 2));
if (image && image.recycle) image.recycle();
toast("探测完成：" + runDir);
