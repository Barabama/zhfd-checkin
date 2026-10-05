// 智汇福大 AutoJs6 配置。仅保存设备和流程参数，不保存账号、Cookie 或 Token。
module.exports = {
    app: {
        packageName: "cn.edu.fzu.fdxypa",
        label: "智汇福大",
        entryText: "晚点名签到",
        serviceText: "我的服务"
    },
    window: {
        start: "21:00",
        end: "23:59",
        launchLeadMinutes: 5
    },
    runtime: {
        dryRun: true,
        // 首次安装/运行时需要在 AutoJs6 中手动授权截图；脚本不会绕过系统授权。
        requestCaptureOnStart: true,
        keepScreenOn: false,
        screenshotPermissionRetries: 1,
        unknownRetries: 3,
        locatingWaitMs: 15000,
        grayWaitMs: 120000,
        clickResultTimeoutMs: 45000,
        settleMs: 1000
    },
    vision: {
        normalizedSize: 315,
        buttonCenterRatio: [0.5, 0.5543],
        buttonBoxRatio: [420 / 1440, 420 / 3168],
        serviceIconCenterRatio: [0.1417, 0.5919],
        greenIconRegionRatio: [0.42, 0.75],
        gray: { maxS: 40, minV: 80, maxV: 230 },
        blue: { minH: 95, maxH: 130, minS: 80, minCoverage: 0.55 },
        green: { minH: 35, maxH: 85, minS: 60, minCoverage: 0.35 },
        // 归一化 315x315 圆心区域内的白字比例；灰态先按无彩色判定。
        readyWhiteRatio: 0.09
    },
    pcapdroid: {
        enabled: false,
        packageName: "com.emanuelef.remote_capture",
        activityName: "com.emanuelef.remote_capture.activities.CaptureCtrl",
        apiKey: "",
        appFilter: "cn.edu.fzu.fdxypa",
        dumpMode: "pcap_file",
        pcapDirectory: "Download/PCAPdroid",
        debug: false
    }
};
