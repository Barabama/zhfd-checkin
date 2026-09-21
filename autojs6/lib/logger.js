var CFG = require("../config");

function now() {
    return new Date();
}

function pad(n) {
    return (n < 10 ? "0" : "") + n;
}

function stamp(d) {
    d = d || now();
    return d.getFullYear() + "-" + pad(d.getMonth() + 1) + "-" + pad(d.getDate()) + "_" +
        pad(d.getHours()) + pad(d.getMinutes()) + pad(d.getSeconds());
}

function iso(d) {
    d = d || now();
    return d.toISOString ? d.toISOString() : String(d);
}

function ensureDir(path) {
    if (!path) return false;
    try {
        // AutoJs6 ensureDir treats its argument as a file path. Supplying a
        // marker below the target therefore creates the target directory itself.
        var marker = files.join(path, ".keep");
        files.ensureDir(marker);
        if (!files.exists(marker)) files.write(marker, "");
        if (files.exists(marker)) files.remove(marker);
        return true;
    } catch (e) {
        try {
            var marker2 = files.join(path, ".keep");
            files.createWithDirs(marker2, "");
            if (files.exists(marker2)) files.remove(marker2);
            return true;
        } catch (e2) { return false; }
    }
}

function createRunDir() {
    var dir = files.join(files.cwd(), "logs", stamp());
    if (!ensureDir(dir)) throw new Error("cannot_create_log_dir: " + dir);
    return dir;
}

function writeText(path, text) {
    try {
        files.createWithDirs(path, String(text));
        return;
    } catch (e) {}
    var parent = files.getDir(path);
    if (!ensureDir(parent)) throw new Error("cannot_create_parent_dir: " + parent);
    files.write(path, String(text));
}

function writeJson(path, value) {
    writeText(path, JSON.stringify(value, null, 2));
}

function log(runDir, message, extra) {
    var line = "[" + iso() + "] " + message;
    if (extra !== undefined) line += " " + JSON.stringify(extra);
    console.log(line);
    if (runDir) {
        var path = files.join(runDir, "events.log");
        try {
            if (files.exists(path)) files.append(path, line + "\n");
            else files.createWithDirs(path, line + "\n");
        } catch (e) {
            // Logging must never abort the actual automation flow.
            console.warn("log_write_failed: " + e);
        }
    }
}

function saveScreenshot(runDir, name, image) {
    if (!image) return null;
    var path = files.join(runDir, name + ".png");
    try {
        ensureDir(runDir);
        images.save(image, path, "png", 100);
        return path;
    } catch (e) {
        console.warn("screenshot_save_failed: " + e);
        return null;
    }
}

module.exports = {
    now: now,
    stamp: stamp,
    iso: iso,
    createRunDir: createRunDir,
    writeJson: writeJson,
    writeText: writeText,
    log: log,
    saveScreenshot: saveScreenshot
};
