var CFG = require("../config");
var vision = require("./vision");

function run(deviceLib, logger, runDir, dryRun) {
    var history = [], clicked = false, successDetected = false, reverified = false, error = null;
    var unknowns = 0, started = new Date();
    function snapshot(name) {
        var image = deviceLib.capture();
        logger.saveScreenshot(runDir, name, image);
        var analysis = vision.analyzeButton(image);
        if (image && image.recycle) image.recycle();
        return analysis;
    }
    while (true) {
        var image = deviceLib.capture();
        var analysis = vision.analyzeButton(image);
        history.push(analysis.state);
        logger.log(runDir, "state", analysis);
        logger.saveScreenshot(runDir, "state_" + history.length + "_" + analysis.state, image);
        if (image && image.recycle) image.recycle();

        if (analysis.state === vision.STATE.SUCCESS) {
            successDetected = true;
            // Re-open the package without clicking and confirm the success state persists.
            // This is a visual re-check, not a network/API forgery or replay.
            try {
                deviceLib.launchTarget();
                var confirmImage = deviceLib.capture();
                var confirmState = vision.analyzeButton(confirmImage);
                logger.saveScreenshot(runDir, "reverify", confirmImage);
                if (confirmImage && confirmImage.recycle) confirmImage.recycle();
                reverified = confirmState.state === vision.STATE.SUCCESS;
                logger.log(runDir, "reverify", confirmState);
            } catch (reverifyError) {
                reverified = false;
                logger.log(runDir, "reverify_error", String(reverifyError));
            }
            break;
        }
        if (!isInWindow()) {
            if (isBeforeWindow()) {
                logger.log(runDir, "before_window", { start: CFG.window.start, now: clockString() });
                waitUntilWindow();
                continue;
            }
            error = "outside_window";
            break;
        }
        if (analysis.state === vision.STATE.GRAY) {
            sleep(CFG.runtime.grayWaitMs);
            continue;
        }
        if (analysis.state === vision.STATE.LOCATING) {
            sleep(CFG.runtime.locatingWaitMs);
            continue;
        }
        if (analysis.state === vision.STATE.READY) {
            if (dryRun) { error = "dry_run_ready"; break; }
            var box = analysis.box;
            click(box.cx, box.cy);
            clicked = true;
            logger.log(runDir, "clicked_ready", { x: box.cx, y: box.cy });
            var deadline = new Date().getTime() + CFG.runtime.clickResultTimeoutMs;
            while (new Date().getTime() < deadline) {
                sleep(3000);
                var after = snapshot("after_click");
                if (after.state === vision.STATE.SUCCESS) { successDetected = true; break; }
                if (after.state === vision.STATE.LOCATING || after.state === vision.STATE.READY) continue;
            }
            if (!successDetected) error = "success_timeout";
            break;
        }
        unknowns++;
        snapshot("unknown_" + unknowns);
        if (unknowns >= CFG.runtime.unknownRetries) { error = "unknown_state"; break; }
        sleep(CFG.runtime.settleMs);
    }
    return { started_at: started.toISOString(), states: history, clicked: clicked,
        success_detected: successDetected, server_state_reverified: reverified, error: error };
}

function clockString() {
    var d = new Date(), hh = ("0" + d.getHours()).slice(-2), mm = ("0" + d.getMinutes()).slice(-2);
    return hh + ":" + mm;
}

function isInWindow() {
    var t = clockString();
    return CFG.window.start <= t && t <= CFG.window.end;
}

function isBeforeWindow() {
    return clockString() < CFG.window.start;
}

function waitUntilWindow() {
    while (isBeforeWindow()) {
        // Use short sleeps so the script remains interruptible and logs stay responsive.
        sleep(Math.min(60000, CFG.runtime.grayWaitMs));
    }
}

module.exports = { run: run, isInWindow: isInWindow, isBeforeWindow: isBeforeWindow };
