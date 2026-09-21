var CFG = require("../config");
var vision = require("./vision");
var ocr = require("./ocr");

function clickService(image) {
    var row = ocr.find(image, [CFG.app.entryText]);
    if (row && ocr.clickBounds(row)) { sleep(1500); return "ocr"; }
    var icon = vision.findGreenServiceIcon(image);
    if (icon) { click(icon.x, icon.y); sleep(3000); return "green_icon"; }
    var x = Math.round(device.width * CFG.vision.serviceIconCenterRatio[0]);
    var y = Math.round(device.height * CFG.vision.serviceIconCenterRatio[1]);
    click(x, y); sleep(3000); return "profile";
}

function openCheckin(image, currentState) {
    if (vision.isCheckinState(currentState.state)) return "already_checkin";
    var method = clickService(image);
    return method;
}

module.exports = { openCheckin: openCheckin };
