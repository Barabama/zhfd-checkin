// Node-only structural smoke test. It does not execute AutoJs6 APIs.
const fs = require("fs");
const path = require("path");
const root = __dirname;
const files = [
  "config.js",
  "lib/device.js",
  "lib/logger.js",
  "lib/navigation.js",
  "lib/ocr.js",
  "lib/pcapdroid.js",
  "lib/state_machine.js",
  "lib/vision.js"
];
for (const file of files) {
  const full = path.join(root, file);
  if (!fs.existsSync(full)) throw new Error(`missing ${file}`);
  require(full);
  console.log(`OK ${file}`);
}
const cfg = require(path.join(root, "config.js"));
if (cfg.app.packageName !== "cn.edu.fzu.fdxypa") throw new Error("unexpected package");
if (cfg.runtime.dryRun !== true) throw new Error("dry-run must remain enabled");
if (cfg.window.start !== "21:30" || cfg.window.end !== "23:59") throw new Error("unexpected window");
console.log("PASS autojs6 structural smoke test");
