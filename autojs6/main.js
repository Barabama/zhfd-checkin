// 智汇福大 AutoJs6 主脚本。
// 默认 dry-run=true；确认真实窗口内 ready 识别稳定后，再在 config.js 中改为 false。
var CFG = require("./config");
var deviceLib = require("./lib/device");
var vision = require("./lib/vision");
var navigation = require("./lib/navigation");
var stateMachine = require("./lib/state_machine");
var logger = require("./lib/logger");
var pcap = require("./lib/pcapdroid");

var runDir = logger.createRunDir();
var result = { device_model: String(device.model || "unknown"), package: CFG.app.packageName,
    started_at: logger.iso(), window: [CFG.window.start, CFG.window.end], states: [],
    clicked: false, success_detected: false, server_state_reverified: false, error: null };

function finish() {
    try { pcap.stop(); } catch (e) { logger.log(runDir, "pcap_stop_error", String(e)); }
    result.finished_at = logger.iso();
    result.run_dir = runDir;
    logger.writeJson(files.join(runDir, "result.json"), result);
    logger.log(runDir, "finish", result);
}

try {
    logger.log(runDir, "start", { dryRun: CFG.runtime.dryRun, pcap: CFG.pcapdroid.enabled });
    if (!deviceLib.requestCapture()) {
        throw new Error("screen_capture_permission_denied");
    }
    deviceLib.wakeAndUnlock();
    var pcapResult = pcap.start(logger.stamp());
    result.pcap_start = pcapResult;
    deviceLib.launchTarget();
    var image = deviceLib.capture();
    var initial = vision.analyzeButton(image);
    logger.saveScreenshot(runDir, "start", image);
    // navigation may need the same screenshot for OCR/visual entry detection.
    var nav = navigation.openCheckin(image, initial);
    if (image && image.recycle) image.recycle();
    result.navigation = nav;
    logger.log(runDir, "navigation", { method: nav });
    if (nav !== "already_checkin") sleep(3000);
    var stateResult = stateMachine.run(deviceLib, logger, runDir, CFG.runtime.dryRun);
    result.states = stateResult.states;
    result.clicked = stateResult.clicked;
    result.success_detected = stateResult.success_detected;
    result.server_state_reverified = stateResult.server_state_reverified;
    result.error = stateResult.error;
} catch (e) {
    result.error = String(e);
    logger.log(runDir, "fatal", String(e));
    try {
        var failure = deviceLib.capture();
        logger.saveScreenshot(runDir, "fatal", failure);
        if (failure && failure.recycle) failure.recycle();
    } catch (e2) {}
} finally {
    finish();
}
console.log(JSON.stringify(result, null, 2));
toast(result.error ? "签到流程结束：" + result.error : "签到流程结束");
