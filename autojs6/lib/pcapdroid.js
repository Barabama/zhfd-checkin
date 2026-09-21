var CFG = require("../config");

function enabled() { return !!CFG.pcapdroid.enabled; }

function launchCommand(extras) {
    var p = CFG.pcapdroid;
    var intent = {
        action: "android.intent.action.VIEW",
        packageName: p.packageName,
        className: p.activityName,
        extras: extras
    };
    // PCAPdroid recommends startActivityForResult so it can identify the caller and
    // enforce its user-consent/API-key policy. Fall back for AutoJs builds without it.
    if (app.startActivityForResult) return app.startActivityForResult(intent);
    app.startActivity(intent);
    return null;
}

function start(runName) {
    if (!enabled()) return { ok: false, skipped: true, reason: "disabled" };
    var p = CFG.pcapdroid;
    var extras = {
        action: "start",
        pcap_dump_mode: p.dumpMode,
        pcap_name: runName + ".pcapng",
        app_filter: p.appFilter,
        pcapng_format: true
    };
    if (p.apiKey) extras.api_key = p.apiKey;
    try {
        var result = launchCommand(extras);
        sleep(1500);
        // CaptureCtrl should return to the caller; explicitly relaunch the target as a safe fallback.
        app.launchPackage(CFG.app.packageName);
        sleep(1000);
        return { ok: true, action: "start", appFilter: p.appFilter, result: result || null };
    } catch (e) {
        console.warn("PCAPdroid start failed: " + e);
        return { ok: false, error: String(e) };
    }
}

function stop() {
    if (!enabled()) return { ok: false, skipped: true, reason: "disabled" };
    try {
        var result = launchCommand({ action: "stop" });
        sleep(1000);
        return { ok: true, action: "stop", result: result || null };
    } catch (e) {
        console.warn("PCAPdroid stop failed: " + e);
        return { ok: false, error: String(e) };
    }
}

function status() {
    if (!enabled()) return { ok: false, skipped: true, reason: "disabled" };
    try {
        var result = launchCommand({ action: "get_status" });
        return { ok: true, action: "get_status", result: result || null };
    } catch (e) {
        return { ok: false, error: String(e) };
    }
}

module.exports = { enabled: enabled, start: start, stop: stop, status: status };
