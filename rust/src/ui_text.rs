//! Every user-visible GUI string, in one place.
//!
//! The GUI never hard-codes user-facing copy: labels, headings, table headers,
//! hints, error explanations and long paragraphs all come from [`Key`] through
//! [`t`]. Adding a language is adding a column to the table, not touching the
//! call sites. `cargo test` checks that both columns are non-empty for every
//! key, so a translation cannot be silently forgotten.
//!
//! Chinese copy stays pure Chinese (product names and file names such as
//! LDPlayer, APK, `ldconsole.exe` are proper nouns); English copy stays pure
//! English. Machine identifiers (`error_code`, `result.json`) are only shown
//! verbatim inside the collapsed technical-details sections.

use crate::domain::{CheckinMode, CheckinState, ExitCode, PageRoute, RunResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Zh,
    En,
}

impl Lang {
    /// Lenient mapping for values that already passed `Config::validate`.
    pub fn from_config(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "en" | "english" => Self::En,
            _ => Self::Zh,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Zh => "zh",
            Self::En => "en",
        }
    }
}

/// The language's own name, shown in the switcher in either UI language.
pub fn language_name(lang: Lang) -> &'static str {
    match lang {
        Lang::Zh => "中文",
        Lang::En => "English",
    }
}

pub struct Text {
    pub zh: &'static str,
    pub en: &'static str,
}

pub struct TermHint {
    pub term: Key,
    pub meaning: Key,
}

macro_rules! keys {
    ($( $variant:ident => ($zh:literal, $en:literal) ),+ $(,)?) => {
        /// A single piece of user-visible copy.
        ///
        /// `ALL` is generated from the same table as the lookup, so the
        /// completeness test cannot drift from the real keys.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Key {
            $($variant),+
        }

        impl Key {
            #[allow(dead_code)] // Tests iterate the whole catalogue.
            pub const ALL: &'static [Key] = &[$(Key::$variant),+];

            fn text(self) -> Text {
                match self {
                    $(Key::$variant => Text { zh: $zh, en: $en }),+
                }
            }
        }
    };
}

keys! {
    // ── Application, navigation, shell ─────────────────────────────────────
    AppTitle => ("智汇福大签到助手", "Zhihui Fuda Check-in Assistant"),
    NavWorkspace => ("工作台", "Workspace"),
    NavRun => ("运行控制", "Run"),
    NavDiagnostics => ("环境诊断", "Environment"),
    NavInstances => ("模拟器实例", "Emulator instances"),
    NavBusiness => ("业务配置", "App setup"),
    NavEvidence => ("任务与证据", "Tasks and evidence"),
    NavProfiles => ("屏幕预设", "Screen presets"),
    NavApp => ("App 安装包", "App package"),
    NavConfig => ("配置文件", "Config file"),
    NavTasks => ("计划任务", "Scheduled tasks"),
    NavLogs => ("运行日志与结果", "Run logs and results"),
    NavHelp => ("帮助", "Help"),
    NavQuickUse => ("快速使用", "Quick start"),
    NavAbout => ("关于", "About"),
    NavSetup => ("首次设置 / 配置修复", "Setup / Repair"),
    PageContents => ("本页目录", "On this page"),
    PageOverview => ("概览", "Overview"),
    PageRunControls => ("运行选项", "Run options"),
    PageLatestResult => ("最近一次结果", "Latest result"),
    PageDiagnosticTools => ("诊断工具", "Diagnostic tools"),
    PageInstanceList => ("实例列表", "Instance list"),
    PagePresetList => ("预设列表", "Preset list"),
    PageInstallPackage => ("安装包设置", "Package settings"),
    PageTaskSettings => ("任务设置", "Task settings"),
    PageTaskWindow => ("运行时间与说明", "Run window and guidance"),
    PageRunHistory => ("运行记录", "Run history"),
    PageEvents => ("事件时间线", "Event timeline"),
    PageUnconfirmed => ("无法确认签到页面", "Could not confirm the check-in page"),
    StatusReady => ("就绪。默认运行方式是试运行。", "Ready. The default mode is a dry run."),
    StatusNeedsSetup => ("需要完成首次配置或修复配置后继续。", "Finish first-time setup or repair the configuration to continue."),
    StatusJobRunning => ("后台任务：{}（运行中）", "Background task: {} (running)"),
    StatusJobBusy => ("已有任务在后台运行，请等待完成。", "A background task is already running. Wait for it to finish."),
    StatusJobStarted => ("{} 已启动，正在后台运行……", "{} started; running in the background…"),
    LanguageLabel => ("界面语言", "Interface language"),
    LanguageSaved => ("界面语言已切换并保存。", "Interface language switched and saved."),
    CommonYes => ("是", "Yes"),
    CommonNo => ("否", "No"),
    CommonUnknown => ("未知", "Unknown"),
    CommonNotRead => ("未读取", "Not read"),
    CommonRequired => ("必须", "Required"),
    CommonNotRequired => ("不需要", "Not needed"),
    CommonFinished => ("完成", "Finished"),
    CommonFailed => ("失败", "Failed"),
    ModeDryRunShort => ("试运行", "Dry run"),
    ModeLiveShort => ("正式运行", "Live run"),

    // ── Home / run control ─────────────────────────────────────────────────
    HomeHeading => ("运行控制", "Run"),
    HomeAboutTitle => ("这个程序做什么", "What this program does"),
    HomeAboutWhat => (
        "自动完成“智汇福大”App 的晚点名签到。",
        "Automates the evening roll-call check-in in the Zhihui Fuda app."
    ),
    HomeAboutDry => (
        "试运行（默认）：只启动 App、浏览页面、读取状态，不点击签到按钮。",
        "Dry run (default): opens the app, navigates, and reads the state without tapping the check-in button."
    ),
    HomeAboutLive => (
        "正式运行：会真实点击签到，需要你在这里确认一次，并通过全部安全检查。",
        "Live run: really taps the check-in button. You confirm once here and every safety gate must pass."
    ),
    HomeAboutUsable => ("现在能不能用：{}", "Can it run now: {}"),
    HomeUsableYes => ("已配置，可以试运行。", "Configured; ready for a dry run."),
    HomeUsableNo => ("需要先完成首次设置。", "First-time setup is not finished."),
    HomeSetupBanner => (
        "首次配置尚未完成：请先进入“首次设置 / 配置修复”填写依赖路径并保存，再运行试运行。",
        "First-time setup is incomplete: open Setup / Repair, fill in the tool paths, save, then run a dry run."
    ),
    HomeOpenSetup => ("打开首次设置 / 配置修复", "Open Setup / Repair"),
    HomeFirstRunTitle => ("第一次使用？按这三步：", "First time here? Three steps:"),
    HomeFirstRunSteps => (
        "1) 启动 LDPlayer　2) 打开 App 并登录　3) 回到这里点“运行试运行”",
        "1) Start LDPlayer　2) Open the app and sign in　3) Come back here and click “Run dry run”"
    ),
    HomeAllInstances => (
        "依次对全部已发现实例运行试运行",
        "Run a dry run on every discovered instance, one after another"
    ),
    HomeCurrentInstance => ("当前实例", "Instance"),
    HomeNotSelected => ("未选择", "Not selected"),
    HomeButtonDiagnostics => ("环境诊断", "Environment check"),
    HomeButtonDryRun => ("运行试运行", "Run dry run"),
    HomeButtonPrepareLive => ("准备正式运行", "Prepare live run"),
    HomeButtonOpenConfig => ("打开配置", "Open config"),
    HomeConfigPath => ("配置文件：{}", "Config file: {}"),
    HomeApkPath => ("安装包：{}", "App package: {}"),
    HomeApkNotSelected => ("未选择", "Not selected"),
    HomeAdvancedCli => (
        "高级用户也可以用命令行：zhfd-checkin.exe run --live --confirm",
        "Advanced users can also use the command line: zhfd-checkin.exe run --live --confirm"
    ),
    HomeLatestResult => ("最近一次结果：{}", "Latest result: {}"),
    HomeLatestPage => (
        "页面：{}；签到类型：{}；签到状态：{}",
        "Page: {}; check-in type: {}; check-in state: {}"
    ),
    HomeSetupIncomplete => (
        "首次配置尚未完成，无法运行；请先保存依赖路径。",
        "First-time setup is incomplete; save the tool paths first."
    ),
    HomeSetupIncompleteLive => (
        "首次配置尚未完成，无法准备正式运行。",
        "First-time setup is incomplete; cannot prepare a live run."
    ),

    // ── Result labels ──────────────────────────────────────────────────────
    ResultNoRun => ("还没有运行记录", "No run yet"),
    ResultSuccess => ("今天已签到", "Checked in today"),
    ResultDryReady => ("可签到，尚未点击", "Ready to check in, not clicked"),
    ResultFieldWorkReady => ("外勤可签到，尚未点击", "Field work ready, not clicked"),
    ResultClickedUnconfirmed => ("已点击，尚未确认成功", "Clicked, success not confirmed"),
    ResultIncomplete => ("本次未完成", "Run incomplete"),
    PageLogin => ("登录页", "Login page"),
    PageHome => ("首页", "Home page"),
    PageServices => ("服务页", "Services page"),
    PageCheckin => ("签到页", "Check-in page"),
    PageCandidateUnconfirmed => ("页面待确认", "Page unconfirmed"),
    PageEvidenceConflict => ("页面证据冲突", "Page evidence conflict"),
    PageStaleRefreshFailed => ("旧页面刷新失败", "Stale page refresh failed"),
    PageStaleRefreshUnconfirmed => ("旧页面刷新未确认", "Stale page refresh unconfirmed"),
    PageAppRestartDialog => ("App 重启提示弹窗", "App restart dialog"),
    PageAppLeftForeground => ("App 已离开前台", "App left the foreground"),
    ModeOrdinary => ("普通晚点名", "Ordinary evening check-in"),
    ModeFieldWork => ("外勤签到", "Field work"),
    ModeUnsupported => ("不支持的签到类型", "Unsupported check-in type"),
    CheckinGray => ("按钮未激活", "Button inactive"),
    CheckinLocating => ("定位中", "Locating"),
    CheckinReady => ("可签到", "Ready to check in"),
    CheckinSuccessToday => ("今天已签到", "Checked in today"),
    CheckinSuccessStale => ("成功页面日期不是今天", "Success page is not from today"),
    CheckinSuccessDateUnknown => ("成功日期未知", "Success date unknown"),
    CheckinFieldWorkReady => ("外勤可签到", "Field work ready"),
    CheckinUnknown => ("未识别", "Not recognized"),
    ButtonGray => ("未激活", "Inactive"),
    ButtonLocating => ("定位中", "Locating"),
    ButtonReady => ("可点击", "Tappable"),
    ButtonSuccess => ("已成功", "Succeeded"),
    ButtonUnknown => ("未识别", "Not recognized"),
    LocationReady => ("已定位", "Location ready"),
    LocationSuccess => ("定位已完成", "Location completed"),
    LoadingYes => ("正在加载", "Loading"),
    LoadingNo => ("已加载", "Loaded"),
    HierarchySucceeded => ("读取成功", "Read successfully"),
    HierarchyFailed => ("读取失败", "Read failed"),
    HierarchyEmpty => ("内容为空或截断", "Empty or truncated"),
    HierarchyMarkersMissing => ("未找到页面标志", "Page markers missing"),
    RefreshAttempted => ("已刷新", "Refreshed"),
    RefreshNotNeeded => ("无需刷新", "Not needed"),
    RefreshConfirmed => ("已确认", "Confirmed"),
    RefreshUnconfirmed => ("未确认", "Unconfirmed"),

    // ── Status card ────────────────────────────────────────────────────────
    StatusCardTitle => ("最近一次运行", "Latest run"),
    StatusCardEmpty => ("还没有运行记录。先运行一次试运行。", "No run yet. Run a dry run first."),
    StatusLabelStatus => ("结果", "Result"),
    StatusLabelRoute => ("页面", "Page"),
    StatusLabelMode => ("签到类型", "Check-in type"),
    StatusLabelButton => ("签到按钮", "Check-in button"),
    StatusLabelLocation => ("定位", "Location"),
    StatusLabelLoading => ("页面加载", "Page loading"),
    StatusLabelHierarchy => ("页面结构", "Page structure"),
    StatusLabelForeground => ("前台 App", "Foreground app"),
    StatusLabelRefresh => ("页面刷新", "Page refresh"),
    StatusOverlay => (
        "检测到遮挡窗口：{}（弹窗或广告，请手动关闭）",
        "Overlay window detected: {} (pop-up or ad; close it manually)"
    ),
    StatusTechnical => ("技术详情（反馈问题时提供）", "Technical details (include when reporting a problem)"),
    StatusErrorCode => ("错误码：{}", "Error code: {}"),
    StatusStage => ("阶段：{}", "Stage: {}"),
    StatusNoErrorCode => ("没有记录错误码", "No error code recorded"),
    StatusRecovery => ("下一步：{}", "Next step: {}"),
    StatusClicked => ("已点击签到按钮", "The check-in button was tapped"),
    StatusNotClicked => ("未点击", "Not tapped"),
    StatusLabelClicked => ("是否点击", "Tapped"),

    // ── Environment checklist ──────────────────────────────────────────────
    EnvTitle => ("准备环境：需要安装什么", "Prepare the environment: what to install"),
    EnvIntro => (
        "下面是全部依赖。没有列出的东西都不需要安装。",
        "These are all the dependencies. Anything not listed is not needed."
    ),
    EnvColNeed => ("需要", "What"),
    EnvColRequired => ("是否必须", "Required"),
    EnvColSource => ("从哪里来", "Where it comes from"),
    EnvWindows => ("Windows 10/11 电脑", "A Windows 10/11 PC"),
    EnvWindowsSrc => ("你的电脑本身", "Your PC"),
    EnvLdplayer => ("雷电模拟器 LDPlayer 14", "LDPlayer 14 emulator"),
    EnvLdplayerSrc => (
        "官网 ldmnq.com 下载安装；安装目录里自带 ldconsole.exe 与 adb.exe",
        "Download from ldmnq.com; the install folder already contains ldconsole.exe and adb.exe"
    ),
    EnvExe => ("本程序（EXE）", "This program (EXE)"),
    EnvExeSrc => ("发布包解压即用，无需安装", "Unzip the release package; no installation"),
    EnvApk => ("智汇福大 App（APK）", "Zhihui Fuda app (APK)"),
    EnvApkSrc => (
        "在模拟器里安装并打开；实例里已装可跳过",
        "Install and open it inside the emulator; skip if the instance already has it"
    ),
    EnvAdb => ("单独安装 ADB", "A separate ADB install"),
    EnvAdbSrc => ("不需要，雷电自带", "Not needed; LDPlayer bundles it"),
    EnvRoot => ("root / VPN / 修改 hosts", "root / VPN / editing hosts"),
    EnvRootSrc => ("不需要", "Not needed"),
    EnvStepsTitle => ("安装顺序", "Install order"),
    EnvStep1 => (
        "1. 安装并打开雷电模拟器。",
        "1. Install and open LDPlayer."
    ),
    EnvStep2 => (
        "2. 新建一个实例并启动，建议分辨率 900x1600、DPI 320；在“设置 → 其他设置”里确认 ADB 调试为“本地连接开启”。",
        "2. Create and start one instance (900x1600 at 320 dpi is recommended); in Settings > Other settings make sure ADB debugging is enabled for local connections."
    ),
    EnvStep3 => (
        "3. 在模拟器里安装并打开智汇福大 App，手动完成登录。",
        "3. Install and open the Zhihui Fuda app inside the emulator and sign in manually."
    ),
    EnvStep4 => (
        "4. 运行本程序，在“首次设置”里选择 ldconsole.exe 与 adb.exe（都在雷电安装目录）。",
        "4. Start this program and, on the Setup page, choose ldconsole.exe and adb.exe (both in the LDPlayer install folder)."
    ),
    EnvNote => (
        "不需要 root、VPN 或修改 hosts；程序不会替你输入账号、密码或验证码。",
        "No root, VPN, or hosts changes are required; the program never types your account, password, or verification code."
    ),

    // ── Terms and relationships ────────────────────────────────────────────
    TermTitle => ("这些名词是什么关系", "How the terms relate"),
    TermInstance => ("实例", "Instance"),
    TermInstanceHint => (
        "LDPlayer 里的一台虚拟手机。",
        "One virtual phone inside LDPlayer."
    ),
    TermIndex => ("编号（index）", "Index"),
    TermIndexHint => (
        "第几台实例，从 0 开始。",
        "Which instance, starting at 0."
    ),
    TermSerial => ("ADB 地址（serial）", "ADB address (serial)"),
    TermSerialHint => (
        "程序连接实例用的地址，通常是 127.0.0.1:5555 + 2×编号。",
        "The address the program uses to connect, usually 127.0.0.1:5555 + 2 × index."
    ),
    TermProfile => ("屏幕预设（Profile）", "Screen preset (profile)"),
    TermProfileHint => (
        "一组“分辨率 + DPI”，决定签到按钮的坐标；与配置文件无关。",
        "A resolution + DPI pair that decides the button coordinates; unrelated to the config file."
    ),
    TermPage => ("页面", "Page"),
    TermPageHint => (
        "当前停在哪个界面：登录页、首页、服务页或签到页。",
        "Which screen the app is on: login, home, services, or check-in."
    ),
    TermCheckinType => ("签到类型", "Check-in type"),
    TermCheckinTypeHint => (
        "普通晚点名或外勤签到。",
        "Ordinary evening check-in or field work."
    ),
    TermCheckinState => ("签到状态", "Check-in state"),
    TermCheckinStateHint => (
        "进行到哪一步：等待、定位中、可签到、成功。",
        "How far it got: waiting, locating, ready, or succeeded."
    ),
    TermResultFile => ("结果文件", "Result file"),
    TermResultFileHint => (
        "一次运行的机器可读结果（result.json）。",
        "The machine-readable outcome of one run (result.json)."
    ),
    TermEvents => ("事件日志", "Event log"),
    TermEventsHint => (
        "一次运行的时间线（events.jsonl），排查问题时先看它。",
        "The timeline of one run (events.jsonl); start here when troubleshooting."
    ),
    TermPortNote => (
        "实例表里的“list2 报告端口”只是雷电自己报的参考值，程序实际用“ADB 地址”连接。",
        "The “list2 reported port” in the instance table is only LDPlayer's own reference value; the program connects with the ADB address."
    ),
    DiagramTitle => ("一次运行的顺序", "The order of one run"),
    Diagram => (
        "页面（在哪一页）→ 签到类型（普通晚点名 / 外勤）→ 签到状态（等待 / 定位中 / 成功）\n→ 结果文件（result.json）与事件日志（events.jsonl）",
        "Page (where the app is) -> check-in type (ordinary / field work) -> check-in state (waiting / locating / succeeded)\n-> result file (result.json) and event log (events.jsonl)"
    ),

    // ── Quick use page ─────────────────────────────────────────────────────
    QuickHeading => ("快速使用", "Quick start"),
    QuickIntro => (
        "按顺序完成首次检查和试运行。",
        "Work through the first check and a dry run in order."
    ),
    QuickPrepareTitle => ("准备环境", "Prepare the environment"),
    QuickSetupTitle => ("首次设置", "First setup"),
    QuickRunTitle => ("运行与看结果", "Run and read results"),
    QuickIssuesTitle => ("常见问题", "Common issues"),
    QuickAdvancedTitle => ("进阶", "Advanced"),
    QuickSetupStep1 => (
        "1. 启动 LDPlayer，并在模拟器里打开智汇福大 App、手动登录。",
        "1. Start LDPlayer, open the Zhihui Fuda app, and sign in manually."
    ),
    QuickSetupStep2 => (
        "2. 在“首次设置 / 配置修复”里填写 ldconsole.exe 与 adb.exe 的路径，点“验证工具并读取实例”，然后保存。",
        "2. On Setup / Repair, fill in ldconsole.exe and adb.exe, click “Validate tools and read instances”, then save."
    ),
    QuickSetupStep3 => (
        "3. 到“环境诊断”确认实例、ADB 连接和屏幕预设；确认 App 在模拟器前台。诊断只读取信息，不会点击。",
        "3. Open Environment and confirm the instance, ADB connection, and screen preset; make sure the app is in the foreground. The check only reads information."
    ),
    QuickSetupStep4 => (
        "4. 回到“运行控制”，选择一个实例，点“运行试运行”。第一次不要勾选全部实例。",
        "4. Return to Run, pick one instance, and click “Run dry run”. Do not select all instances the first time."
    ),
    QuickSetupNoneNeeded => (
        "已经设置过？直接跳到“运行与看结果”。",
        "Already configured? Skip to “Run and read results”."
    ),
    QuickRunDrySuccess => (
        "试运行成功，就是看到“可签到”。这时程序没有点击，也不等于签到成功。",
        "A successful dry run means the state reads “ready to check in”. Nothing was tapped, and this is not a check-in."
    ),
    QuickRunLive => (
        "正式运行会真实点击。先点“准备正式运行”，在弹窗里确认一次；之后仍要通过全部安全检查。",
        "A live run really taps. Click “Prepare live run” and confirm once in the dialog; every safety gate still applies."
    ),
    QuickRunFailure => (
        "失败时先看“结果”和“下一步”里的中文说明，不需要理解技术字段。",
        "On failure, read the plain-language result and next step; you do not need the technical fields."
    ),
    QuickOpenLogs => ("打开运行日志与结果", "Open run logs and results"),
    QuickOpenRun => ("返回运行控制", "Back to Run"),
    QuickOpenDiagnostics => ("打开环境诊断", "Open Environment"),
    QuickOpenSetup => ("打开首次设置", "Open Setup"),
    QuickIssueDevice => (
        "设备没连上：确认模拟器已启动、实例编号和 ADB 地址正确，再运行环境诊断。",
        "Device not found: make sure the emulator is running and the instance index and ADB address are correct, then run the environment check."
    ),
    QuickIssueLogin => (
        "需要登录：在模拟器里手动登录。若你显式开启过一键登录，程序会先尝试一次，失败就停下并提示。",
        "Sign-in required: sign in manually inside the emulator. If you enabled one-click login, the program tries once and stops on failure."
    ),
    QuickIssueDialog => (
        "有弹窗或广告：在模拟器里手动关闭后重试。程序不会自动关闭、同意或点击弹窗。",
        "Pop-up or ad: close it manually in the emulator and retry. The program never closes, accepts, or taps pop-ups."
    ),
    QuickIssueMore => (
        "页面未确认、页面证据冲突、定位超时、配置损坏等情况，见“进阶”。",
        "Unconfirmed pages, conflicting evidence, location timeouts, and damaged configs are covered under “Advanced”."
    ),
    QuickAdvancedInstances => (
        "多实例：确认单实例试运行正常后，再勾选“全部已发现实例”；每个实例单独记录结果，一个失败不会抹掉其他结果。",
        "Multiple instances: after one instance works, enable “all discovered instances”. Each instance records its own result; one failure does not erase the others."
    ),
    QuickAdvancedTasks => (
        "计划任务：建议保持试运行。不要把正式运行设为无人值守任务。",
        "Scheduled tasks: keep them on dry run. Do not schedule unattended live runs."
    ),
    QuickAdvancedData => (
        "数据位置：日志、截图和报告都在当前数据目录；迁移前确认目标目录可写，迁移会先备份，源目录不删除。",
        "Data location: logs, screenshots, and reports live in the current data directory. Before moving it, make sure the target is writable; the move backs up first and keeps the source."
    ),
    QuickAdvancedShare => (
        "导出或分享诊断资料前，请检查其中的本机路径、实例信息和截图。",
        "Before exporting or sharing diagnostics, check for local paths, instance details, and screenshots."
    ),
    QuickAdvancedBroken => (
        "配置损坏：进入“首次设置 / 配置修复”。原文件不会被静默覆盖。",
        "Damaged config: open Setup / Repair. The original file is never silently overwritten."
    ),
    QuickOpenTasks => ("打开计划任务", "Open scheduled tasks"),
    QuickOpenConfig => ("打开配置文件", "Open config file"),

    // ── Safety table ───────────────────────────────────────────────────────
    SafetyTitle => ("安全边界", "Safety boundaries"),
    SafetyDoTitle => ("程序会做", "The program does"),
    SafetyDontTitle => ("程序不会做", "The program does not"),
    SafetyDoLaunch => ("启动 App、浏览页面、读取状态", "Open the app, navigate, and read state"),
    SafetyDoTap => (
        "正式运行时点击一次签到按钮",
        "Tap the check-in button once in a live run"
    ),
    SafetyDoLogin => (
        "在你显式开启后，于登录页执行一次一键登录",
        "Perform one-click login only after you explicitly enable it"
    ),
    SafetyDoWrite => ("把结果写到本机日志目录", "Write results to the local log folder"),
    SafetyDontCredentials => ("输入账号、密码或验证码", "Type your account, password, or verification code"),
    SafetyDontPopups => (
        "连续点击，或自动关闭广告和弹窗",
        "Tap repeatedly, or close ads and pop-ups automatically"
    ),
    SafetyDontStore => ("保存或上传你的凭据", "Store or upload your credentials"),
    SafetyDontUpload => ("上传截图、定位或账号", "Upload screenshots, location, or account data"),

    // ── Config page ────────────────────────────────────────────────────────
    ConfigHeading => ("配置文件（config.toml）", "Config file (config.toml)"),
    ConfigIntro => (
        "这里修改的是程序设置文件，不是上面的“屏幕预设”。",
        "This page edits the program's settings file, not the screen presets."
    ),
    ConfigSectionDeps => ("依赖与路径", "Tools and paths"),
    ConfigSectionFieldWork => ("外勤签到授权", "Field-work permission"),
    ConfigSectionLogin => ("登录页自动登录", "Automatic login"),
    ConfigSectionToml => ("当前配置内容", "Current settings"),
    ConfigPath => ("配置文件：{}", "Config file: {}"),
    ConfigScan => ("扫描 LDPlayer / ADB 依赖", "Scan for LDPlayer / ADB"),
    ConfigSaveFound => ("保存选中的发现路径", "Save the selected discovered paths"),
    ConfigOpenToml => ("打开 config.toml", "Open config.toml"),
    ConfigTomlHint => (
        "Windows 路径推荐使用 TOML 单引号写法，无需双写反斜杠；正斜杠路径也可以。",
        "For Windows paths, TOML single quotes are recommended (no double backslashes); forward slashes also work."
    ),
    ConfigDepsHeading => ("依赖发现结果", "Tool discovery results"),
    ConfigDepNone => ("没有可选的已安装候选", "No installed candidates found"),
    ConfigDepCandidates => ("{} 候选诊断", "{} candidate details"),
    ConfigDepOmitted => ("另有 {} 个候选未展开。", "{} more candidates not shown."),
    ConfigFieldWorkHeading => ("外勤签到授权", "Field-work permission"),
    ConfigFieldWorkCheckbox => (
        "允许正式运行点击外勤签到按钮（高风险，需要明确确认）",
        "Allow live runs to tap the field-work button (high risk; requires explicit confirmation)"
    ),
    ConfigFieldWorkNote => (
        "默认关闭。启用后仍需你逐次确认，并继续受校准屏幕预设、时间窗口、目标 App 前台、连续就绪和点击前复核保护；未知状态和证据冲突仍会停止。",
        "Off by default. Even when enabled, each run still needs your confirmation and must pass the calibrated preset, time window, foreground app, stable-ready, and pre-tap checks; unknown states and conflicting evidence still stop the run."
    ),
    ConfigLoginHeading => ("登录页自动登录（一键登录）", "Automatic login (one click)"),
    ConfigLoginRisk => (
        "高风险：只在登录页使用 App 记住的上次账号，不输入、不保存账号、密码或验证码。",
        "High risk: only uses the account the app already remembers on the login page. It never types or stores an account, password, or verification code."
    ),
    ConfigLoginAllow => (
        "允许正式运行在登录页执行一次一键登录",
        "Allow a live run to perform one-click login on the login page"
    ),
    ConfigLoginTerms => (
        "允许程序在登录页代勾选《用户协议》《隐私协议》（代你作出同意）",
        "Allow the program to tick the User and Privacy agreements on your behalf"
    ),
    ConfigAccountLabel => ("期望账号（可留空）", "Expected account (optional)"),
    ConfigAccountUnset => ("未设置", "Not set"),
    ConfigAccountSet => ("已设置（默认隐藏）", "Set (hidden by default)"),
    ConfigShowPlain => ("显示明文", "Show plain text"),
    ConfigLoginTimeout => ("登录后等待秒数（5 到 120）", "Seconds to wait after login (5-120)"),
    ConfigLoginNote => (
        "默认关闭。即使开启，仍要求目标 App 在前台、没有外部窗口、只有一个“一键登录”按钮、账号匹配、协议复选框经过视觉确认和点击前二次观测。试运行永不点击，一次运行最多一次登录点击。",
        "Off by default. Even when enabled, the target app must be in the foreground, no foreign window may be present, exactly one one-click-login button must exist, the account must match, and the consent checkbox must be visually confirmed and re-checked before tapping. Dry runs never tap; one run performs at most one login tap."
    ),
    ConfigTomlHeading => (
        "当前配置内容（账号与通知地址已脱敏；明文请用“打开配置所在目录”查看）",
        "Current settings (account and notification address redacted; use “Open config folder” for the plain file)"
    ),
    ConfigScanFirst => (
        "请先扫描依赖，再选择并保存候选路径。",
        "Scan for dependencies first, then select and save candidate paths."
    ),
    ConfigPathsSaved => (
        "已保存选中的 LDPlayer / ADB 路径。",
        "Saved the selected LDPlayer / ADB paths."
    ),
    ConfigOpened => ("已打开 config.toml。", "config.toml opened."),
    FieldWorkEnabled => (
        "已启用外勤正式运行；请确认页面、实例和定位后再执行。",
        "Live field work enabled; confirm the page, instance, and location before running."
    ),
    FieldWorkDisabled => (
        "已关闭外勤正式运行；外勤页面只允许试运行观察。",
        "Live field work disabled; field-work pages can only be observed in a dry run."
    ),
    AutoLoginEnabled => (
        "已启用自动登录；仍需单独授权代勾选协议。",
        "Automatic login enabled; ticking the agreements still needs separate consent."
    ),
    AutoLoginDisabled => (
        "已关闭自动登录；登录页恢复为手动登录。",
        "Automatic login disabled; the login page returns to manual sign-in."
    ),
    TermsGranted => (
        "已授权代勾选登录协议；该授权具有法律同意含义。",
        "Consent to tick the login agreements granted; this consent has legal meaning."
    ),
    TermsRevoked => (
        "已撤销代勾选协议授权；自动登录将拒绝执行。",
        "Consent to tick the agreements revoked; automatic login will refuse to run."
    ),
    AccountSaved => ("已保存期望账号。", "Expected account saved."),

    // ── Setup / Repair page ────────────────────────────────────────────────
    SetupRepairHeading => ("配置修复", "Repair configuration"),
    SetupFirstHeading => ("首次设置", "First-time setup"),
    SetupConfigPath => ("配置文件：{}", "Config file: {}"),
    SetupDataRoot => ("数据目录：{}", "Data folder: {}"),
    SetupOrigin => ("配置来源：{}", "Config source: {}"),
    SetupOriginExplicit => ("显式指定的文件或修复路径", "An explicitly specified file or repair path"),
    SetupOriginPortable => ("程序所在目录（便携模式）", "The program folder (portable mode)"),
    SetupOriginLocalAppData => ("本地应用数据目录", "Local application data folder"),
    SetupOriginRecorded => ("已迁移的数据目录", "A previously migrated data folder"),
    SetupConfigProblem => ("配置问题：{}", "Config problem: {}"),
    SetupConfigProblemNote => (
        "原文件未被覆盖。请重新载入、选择另一份配置，或在确认后显式恢复安全默认配置。",
        "The original file was not overwritten. Reload it, choose another config, or explicitly restore safe defaults after confirming."
    ),
    SetupCopyError => ("复制错误信息", "Copy error text"),
    SetupLastAction => ("上次操作：{}", "Last action: {}"),
    SetupDepsHeading => ("1. 依赖工具路径", "1. Tool paths"),
    SetupHintLdconsole => ("雷电模拟器控制台程序", "LDPlayer console executable"),
    SetupHintAdb => ("安卓调试工具程序", "Android debug bridge executable"),
    SetupBrowse => ("浏览…", "Browse…"),
    SetupValidate => ("验证工具并读取实例", "Validate tools and read instances"),
    SetupValidating => ("正在验证……", "Validating…"),
    SetupSave => ("确认并保存路径", "Confirm and save paths"),
    SetupOpenDir => ("打开配置所在目录", "Open config folder"),
    SetupEmptyPaths => (
        "请先填写 ldconsole.exe 和 adb.exe 的路径；依赖为空时不会解除首次设置。",
        "Fill in both ldconsole.exe and adb.exe; empty paths keep first-time setup open."
    ),
    SetupSaved => ("依赖配置已保存。", "Tool paths saved."),
    SetupRestoreHeading => ("2. 恢复与重新载入", "2. Restore and reload"),
    SetupReload => ("重新载入磁盘配置", "Reload config from disk"),
    SetupChooseOther => ("选择另一份 config.toml…", "Choose another config.toml…"),
    SetupResetDefaults => ("恢复安全默认配置…", "Restore safe defaults…"),
    SetupResetConfirm => ("再次点击确认：备份并恢复安全默认", "Click again to confirm: back up and restore safe defaults"),
    SetupMigrationHeading => ("3. 数据目录迁移", "3. Move the data folder"),
    SetupMigrationNote => (
        "迁移会复制配置、日志、截图、报告和备份；只在目标配置验证通过后切换，源目录不会删除。",
        "The move copies config, logs, screenshots, reports, and backups; it switches only after the target config validates, and never deletes the source."
    ),
    SetupMigrationTargetHint => ("目标数据目录", "Target data folder"),
    SetupChooseDir => ("选择目录…", "Choose folder…"),
    SetupMigrate => ("迁移并切换", "Move and switch"),
    SetupLoginNote => (
        "默认手动登录：程序不会输入账号、密码或验证码。只有同时开启自动登录与协议授权后，正式运行才会在登录页执行一次一键登录。首次试运行建议只选一个实例。",
        "Manual sign-in is the default: the program never types an account, password, or verification code. Only when both automatic login and consent are enabled may a live run perform one-click login on the login page. For the first dry run, use one instance."
    ),
    SetupErrorCopied => ("配置错误信息已复制。", "Error text copied."),
    SetupAdbSuggested => (
        "已建议同目录下的 adb.exe；请确认后再保存。",
        "Suggested adb.exe from the same folder; confirm before saving."
    ),
    SetupPathsIncomplete => ("依赖路径不完整，配置未保存。", "Tool paths are incomplete; the config was not saved."),
    SetupSaveFailed => (
        "保存失败，现有配置保持不变：{}",
        "Save failed; the existing config is unchanged: {}"
    ),
    SetupOpenDirFailed => ("打开配置目录失败：{}", "Failed to open the config folder: {}"),
    SetupReloadedEmpty => (
        "已从磁盘重新载入；依赖路径仍为空，请完成设置。",
        "Reloaded from disk; the tool paths are still empty. Finish setup."
    ),
    SetupReloaded => (
        "已从磁盘重新载入并验证配置。",
        "Reloaded and validated the config from disk."
    ),
    SetupReloadFailed => (
        "未载入，保留当前有效配置：{}",
        "Not reloaded; the current valid config was kept: {}"
    ),
    SetupSwitched => ("已切换到所选配置文件。", "Switched to the selected config file."),
    SetupAltUnavailable => ("所选配置不可用：{}", "The selected config is not usable: {}"),
    SetupResetDone => (
        "已备份原配置并恢复安全默认值；请重新设置依赖路径。",
        "Backed up the original config and restored safe defaults; set the tool paths again."
    ),
    SetupResetFailed => (
        "恢复失败，原配置保持不变：{}",
        "Restore failed; the original config is unchanged: {}"
    ),
    SetupResetNeedsConfirm => (
        "恢复默认配置需要再次确认。",
        "Restoring defaults needs one more confirmation."
    ),
    SetupChooseTarget => ("请先选择目标数据目录。", "Choose a target data folder first."),
    MigrationTitle => ("确认迁移数据目录", "Confirm data folder move"),
    MigrationCurrent => ("当前：{}", "Current: {}"),
    MigrationTarget => ("目标：{}", "Target: {}"),
    MigrationWarning => (
        "迁移前会先备份当前配置；源目录不会删除。确认继续？",
        "The current config is backed up first; the source is never deleted. Continue?"
    ),
    MigrationConfirm => ("确认迁移", "Move"),
    CommonCancel => ("取消", "Cancel"),
    MigrationCancelled => ("已取消数据目录迁移。", "Data folder move cancelled."),
    MigrationRecorded => (
        "数据目录已迁移到 {}；源目录保留未删除。",
        "Data folder moved to {}; the source folder was kept."
    ),
    MigrationRecordFailed => (
        "数据已复制到 {}，但无法记录新数据目录；重启后会回到旧目录：{}",
        "Data was copied to {} but the new folder could not be recorded; a restart will return to the old folder: {}"
    ),
    MigrationIncomplete => (
        "数据目录迁移未完成（记录新目录失败）。",
        "Data folder move incomplete (recording the new folder failed)."
    ),
    MigrationFailed => (
        "迁移失败，仍使用原数据目录 {}：{}",
        "Move failed; still using the original data folder {}: {}"
    ),
    ScanRunning => ("依赖扫描正在运行。", "A dependency scan is already running."),
    ScanStarted => ("正在扫描 LDPlayer / ADB 依赖……", "Scanning for LDPlayer / ADB…"),
    ValidationRunning => ("依赖验证正在运行。", "Dependency validation is already running."),
    ValidationStarted => (
        "正在验证 LDPlayer / ADB 并读取实例……",
        "Validating LDPlayer / ADB and reading instances…"
    ),
    ValidationSummary => (
        "{}（已验证工具并读取到实例；尚未写入配置）",
        "{} (tools validated and instances read; not written to the config yet)"
    ),
    ValidationFailed => ("验证失败：{}", "Validation failed: {}"),
    ValidationPassed => ("依赖验证通过。", "Dependency validation passed."),
    StatusDone => ("已完成。", "Done."),
    StatusInstancesFound => ("检测到 {} 个实例。", "{} instances found."),
    StatusReportExported => ("诊断报告已导出：{}", "Diagnostic report exported: {}"),
    StatusApkPathSaved => ("已保存安装包路径：{}", "App package path saved: {}"),
    StatusLogsDirOpened => ("已打开日志目录。", "Log folder opened."),
    StatusReportOpened => ("已打开诊断报告。", "Diagnostic report opened."),
    StatusExportReportFirst => ("请先导出诊断报告。", "Export a diagnostic report first."),
    StatusPathCopied => ("路径已复制。", "Path copied."),
    StatusEvidenceCopied => ("选中的结果或证据已复制。", "Selected result or evidence copied."),
    StatusNothingToCopy => ("选中目录没有可复制的证据文件。", "The selected folder has no evidence file to copy."),
    StatusEvidenceExported => ("证据已导出：{}", "Evidence exported: {}"),
    StatusEventLogExported => ("事件日志已导出：{}", "Event log exported: {}"),
    StatusCanceledLive => ("已取消正式运行。", "Live run cancelled."),
    StatusCanceledSync => ("已取消屏幕预设同步。", "Screen preset sync cancelled."),
    StatusCanceledApk => ("已取消安装包安装。", "App package install cancelled."),
    LiveTitle => ("确认正式运行", "Confirm live run"),
    LiveScopeAll => ("全部已启用实例", "All enabled instances"),
    LiveScopeOne => ("当前选择的实例", "The selected instance"),
    LiveScope => ("范围：{}", "Scope: {}"),
    LiveConfigPath => ("配置文件：{}", "Config file: {}"),
    LiveDataRoot => ("数据目录：{}", "Data folder: {}"),
    LiveInstanceIndex => ("实例编号：{}", "Instance index: {}"),
    LiveWarning => (
        "将执行真实签到点击。普通签到仍会检查已标定的屏幕预设、签到时间窗口、前台 App、连续就绪状态和点击前二次确认；外勤签到还要求在配置文件中显式开启外勤授权，并使用外勤页面自己的按钮。程序不会把外勤降级为普通签到。",
        "This performs a real check-in tap. Ordinary check-in still requires a calibrated screen preset, the check-in window, the foreground app, stable readiness, and a second confirmation before tapping. Field work additionally requires the field-work permission in the config file and uses its own button. Field work is never downgraded to an ordinary check-in."
    ),
    LiveConfirmNote => (
        "请确认设备、实例和当前页面均正确；确认后将调用与命令行相同的正式运行逻辑。",
        "Make sure the device, instance, and current page are correct; confirmation runs the same live logic as the command line."
    ),
    LiveConfirm => ("确认正式运行", "Confirm live run"),
    LiveChanged => (
        "配置、数据目录或实例在确认前发生变化；正式运行未启动，请重新准备。",
        "The config, data folder, or instance changed before confirmation; the live run did not start. Prepare it again."
    ),
    SyncTitle => ("确认同步屏幕预设", "Confirm screen preset sync"),
    SyncWarning => (
        "将修改 LDPlayer 的分辨率和 DPI，并重启实例：{}",
        "This changes LDPlayer's resolution and DPI and restarts the instance: {}"
    ),
    SyncNote => (
        "同步会先备份已有配置；执行期间请不要手动操作模拟器。确认继续？",
        "The sync backs up the existing config first; do not touch the emulator while it runs. Continue?"
    ),
    SyncConfirm => ("确认同步", "Sync"),
    SyncJob => ("同步屏幕预设 {}", "Sync screen preset {}"),

    // ── Diagnostics ────────────────────────────────────────────────────────
    DiagHeading => ("环境诊断", "Environment check"),
    DiagIntro => (
        "这里只读取依赖和设备信息，不会签到、不会点击。",
        "This page only reads dependencies and device information; it never checks in or taps."
    ),
    DiagRedetect => ("重新检测 LDPlayer / ADB", "Re-check LDPlayer / ADB"),
    DiagExportReport => ("导出诊断报告", "Export diagnostic report"),
    DiagDumpHierarchy => ("导出当前页面的页面结构", "Export the current page structure"),
    DiagAnalyzeImage => ("分析截图…", "Analyze a screenshot…"),
    DiagRefreshResult => ("刷新最近运行结果", "Refresh latest result"),
    DiagNote => (
        "诊断和报告导出不会执行签到点击。报告不包含令牌、Cookie 或定位地址。",
        "Diagnostics and report export never tap. Reports contain no tokens, cookies, or location addresses."
    ),
    DiagReportPath => ("最近导出的报告：{}", "Latest exported report: {}"),
    DiagLatestLog => ("最近一次运行日志：{}", "Latest run log: {}"),
    DiagNoRun => ("还没有运行记录。", "No run recorded yet."),
    DiagMode => ("运行方式：{}", "Run mode: {}"),
    DiagProfile => ("屏幕预设：{}", "Screen preset: {}"),
    DiagPage => ("页面：{}", "Page: {}"),
    DiagCheckinType => ("签到类型：{}", "Check-in type: {}"),
    DiagCheckinState => ("签到状态：{}", "Check-in state: {}"),
    DiagClicked => ("点击：{}", "Tapped: {}"),
    DiagAutoLogin => ("自动登录：{}", "Automatic login: {}"),
    DiagPageDate => ("页面日期：{}", "Page date: {}"),
    DiagError => ("错误：{}（{}）", "Error: {} ({})"),
    DiagFieldWorkReady => (
        "外勤签到状态已识别为可签到；仅试运行观察，没有点击。",
        "Field work reads as ready; this was a dry run, so nothing was tapped."
    ),
    DiagNoError => ("没有记录运行错误", "No run error recorded"),
    DiagStage => ("失败阶段：{}", "Failure stage: {}"),
    DiagAction => ("下一步：{}", "Next step: {}"),
    DiagHistory => ("状态变化：{}", "State history: {}"),
    DiagSerial => ("ADB 地址：{}", "ADB address: {}"),
    DiagDefaultNote => (
        "默认运行方式是试运行。只有普通签到页面能进入正式点击策略；外勤签到和未知类型只做无点击观察。",
        "The default mode is a dry run. Only the ordinary check-in page can enter the live tap policy; field work and unknown types are observed without tapping."
    ),
    DiagDumpDone => (
        "完成，页面结构长度 {}",
        "Finished; page structure length {}"
    ),
    DiagImageState => ("截图分析：状态 {}", "Screenshot analysis: state {}"),
    DiagImageCenter => ("按钮中心 ({}, {})", "button center ({}, {})"),
    DiagResultRefreshed => ("最近运行结果已刷新。", "Latest result refreshed."),

    // ── Instances ──────────────────────────────────────────────────────────
    InstHeading => ("模拟器实例", "Emulator instances"),
    InstIntro => (
        "一台实例就是 LDPlayer 里的一台虚拟手机；编号是它的序号，ADB 地址是程序连接它用的地址。",
        "An instance is one virtual phone in LDPlayer; the index is its number and the ADB address is what the program connects to."
    ),
    InstRefresh => ("刷新实例列表", "Refresh instance list"),
    InstColIndex => ("编号", "Index"),
    InstColName => ("名称", "Name"),
    InstColRunning => ("运行中", "Running"),
    InstColSerial => ("ADB 地址", "ADB address"),
    InstColPort => ("雷电报告的端口（参考）", "LDPlayer-reported port (reference)"),
    InstColGeometry => ("尺寸/DPI", "Size / DPI"),
    InstColProfile => ("屏幕预设", "Screen preset"),
    InstColEnabled => ("已启用", "Enabled"),
    InstPortHover => (
        "这是 ldconsole 自己报告的参考字段，不是程序连接设备用的 ADB 地址。",
        "This is LDPlayer's own reference field, not the ADB address the program connects to."
    ),

    // ── Profiles ───────────────────────────────────────────────────────────
    ProfilesHeading => ("屏幕预设", "Screen presets"),
    ProfilesIntro => (
        "屏幕预设是一组“分辨率 + DPI”，决定签到按钮的坐标；它与“配置文件”无关。",
        "A screen preset is a resolution + DPI pair that decides the button coordinates; it is unrelated to the config file."
    ),
    ProfilesCurrent => ("当前实例编号：{}", "Current instance index: {}"),
    ProfilesDetect => ("检测当前模拟器分辨率", "Detect the emulator's resolution"),
    ProfilesNote => (
        "已标定的预设排在前面。未标定的预设仍可同步，但不能用于正式点击。",
        "Calibrated presets are listed first. Uncalibrated presets can still be synced but cannot be used for live taps."
    ),
    ProfilesColId => ("预设 ID", "Preset ID"),
    ProfilesColSize => ("尺寸", "Size"),
    ProfilesColDpi => ("像素密度（DPI）", "DPI"),
    ProfilesColOrientation => ("方向", "Orientation"),
    ProfilesColEligibility => ("能否正式点击", "Usable for live taps"),
    ProfilesColAction => ("操作", "Action"),
    ProfilesCalibrated => ("已标定", "Calibrated"),
    ProfilesDryOnly => ("仅试运行", "Dry run only"),
    ProfilesSync => ("准备同步", "Prepare sync"),
    ProfilesHistory => ("（历史）", " (legacy)"),
    ProfilesDetectJob => ("检测屏幕预设", "Detect screen preset"),
    ProfilesDetectMatch => (
        "当前 {} {}x{}@{}，匹配屏幕预设 {}",
        "Current {} {}x{}@{}; matched preset {}"
    ),
    ProfilesDetectNoMatch => (
        "当前 {} {}x{}@{}，没有精确匹配的屏幕预设",
        "Current {} {}x{}@{}; no exact screen preset match"
    ),
    OrientationPortrait => ("竖屏", "Portrait"),
    OrientationLandscape => ("横屏", "Landscape"),

    // ── App page ───────────────────────────────────────────────────────────
    AppHeading => ("应用管理", "App management"),
    AppPackage => ("目标包名：{}", "Target package: {}"),
    AppInstalledVersion => ("当前已安装版本", "Installed version"),
    AppNotRead => ("尚未读取", "Not read yet"),
    AppRefreshVersion => ("刷新版本", "Refresh version"),
    AppApkPath => ("APK 路径", "APK path"),
    AppBrowse => ("浏览 APK…", "Browse APK…"),
    AppSavePath => ("保存 APK 路径", "Save APK path"),
    AppInstall => ("安装/更新选中的 APK", "Install / update the selected APK"),
    AppLatestInstall => ("最近一次安装结果：{}", "Latest install result: {}"),
    AppNote => (
        "当前版本支持标准 .apk；不会自动下载，也不会修改账号或设备身份。安装前必须确认，安装后会校验目标包名和版本。",
        "Only standard .apk files are supported. The program never downloads apps or changes accounts or device identity. Installation always asks for confirmation and then verifies the package name and version."
    ),
    ApkTitle => ("确认安装安装包", "Confirm app package install"),
    ApkPath => ("安装包：{}", "Package: {}"),
    ApkPackageName => ("安装包包名：{}", "Package name: {}"),
    ApkVersionName => ("安装包版本名：{}", "versionName: {}"),
    ApkVersionCode => ("安装包版本号：{}", "versionCode: {}"),
    ApkWarning => (
        "将连接当前模拟器，并使用 adb install -r 安装或更新安装包。",
        "This connects to the current emulator and runs adb install -r to install or update the package."
    ),
    CommonConfirm => ("确认", "Confirm"),
    ApkJob => ("安装安装包", "Install app package"),
    ApkDone => (
        "完成：{} {}；adb 输出：{}",
        "Finished: {} {}; adb output: {}"
    ),
    AppPackageMismatch => (
        "安装包包名不匹配：实际 {}，目标 {}",
        "Package name mismatch: found {}, expected {}"
    ),
    AppMissing => ("安装包不存在：{}", "The package does not exist: {}"),
    AppChooseFirst => ("请先选择安装包。", "Choose a package first."),

    // ── Scheduled tasks ────────────────────────────────────────────────────
    TasksHeading => ("计划任务", "Scheduled tasks"),
    TasksIntro => (
        "计划任务默认创建为试运行；正式任务必须显式勾选并再次确认。",
        "Scheduled tasks are created as dry runs by default; a live task needs an explicit checkbox and a second confirmation."
    ),
    TasksName => ("任务名称", "Task name"),
    TasksLive => ("创建正式运行任务（需要显式确认）", "Create a live task (explicit confirmation required)"),
    TasksAllInstances => ("任务依次运行全部已启用实例", "The task runs every enabled instance in order"),
    TasksQuery => ("查询任务", "Query task"),
    TasksCreate => ("创建任务", "Create task"),
    TasksDelete => ("删除任务", "Delete task"),
    TasksLiveWarning => (
        "正式计划任务会每天在签到窗口开始时执行真实点击；必须再次确认。",
        "A live scheduled task taps for real every day at the start of the check-in window; it must be confirmed again."
    ),
    TasksConfirmLive => ("确认创建正式计划任务", "Confirm live scheduled task"),
    TasksWindow => ("签到时间窗口：{} 至 {}", "Check-in window: {} to {}"),
    TasksLiveNeedsConfirm => (
        "正式计划任务需要再次确认；请点击下方确认按钮。",
        "A live scheduled task needs one more confirmation; click the confirm button below."
    ),
    TasksGuiNote => (
        "界面不提供直接正式签到的按钮；计划任务的正式运行仍受全部安全检查保护。",
        "The interface has no direct live check-in button; live scheduled runs are still protected by every safety gate."
    ),

    // ── Logs ───────────────────────────────────────────────────────────────
    LogsHeading => ("运行日志与结果", "Run logs and results"),
    LogsRefresh => ("刷新列表", "Refresh list"),
    LogsOpenDir => ("打开日志目录", "Open log folder"),
    LogsExportReport => ("导出诊断报告", "Export diagnostic report"),
    LogsOpenReport => ("打开最近报告", "Open latest report"),
    LogsFilter => ("筛选", "Filter"),
    LogsFilterHint => ("类型、状态、摘要或路径", "Type, status, summary, or path"),
    LogsSelected => ("已选：{}", "Selected: {}"),
    LogsOpen => ("打开目录", "Open folder"),
    LogsCopyPath => ("复制路径", "Copy path"),
    LogsCopyEvidence => ("复制结果/证据", "Copy result / evidence"),
    LogsExportSelected => ("导出选中的结果文件", "Export the selected result file"),
    LogsExportEvents => ("导出事件日志", "Export event log"),
    LogsEmpty => ("还没有运行或同步记录。", "No run or sync records yet."),
    LogsNoMatch => ("没有匹配当前筛选条件的记录。", "No records match the filter."),
    LogsColKind => ("类型", "Type"),
    LogsColStatus => ("状态", "Status"),
    LogsColSummary => ("摘要", "Summary"),
    LogsColDir => ("日志目录", "Log folder"),
    LogsSelectHint => ("选择一条记录以查看和导出结果。", "Select a record to view and export its result."),
    LogsPreview => ("证据预览", "Evidence preview"),
    LogsTimeline => ("事件时间线（前台 / 页面结构 / 页面识别 / 导航）", "Event timeline (foreground / page structure / page identity / navigation)"),
    LogsNoEvents => ("选中目录没有可展示的事件日志。", "The selected folder has no event log to show."),
    LogsColTime => ("时间", "Time"),
    LogsColEvent => ("事件", "Event"),
    LogKindRun => ("运行", "Run"),
    LogKindSync => ("预设同步", "Preset sync"),
    LogKindBatch => ("批量运行", "Batch run"),
    LogStatusSuccess => ("成功", "Success"),
    LogStatusReady => ("可签到", "Ready"),
    LogStatusFieldReady => ("外勤可签到", "Field work ready"),
    LogStatusBlocked => ("被安全检查阻止", "Blocked by safety check"),
    LogStatusFailed => ("失败", "Failed"),
    LogStatusIncomplete => ("未完成", "Incomplete"),

    // ── About ──────────────────────────────────────────────────────────────
    AboutHeading => ("关于", "About"),
    AboutVersion => ("版本：{}", "Version: {}"),
    AboutLanguage => ("界面语言", "Interface language"),
    AboutLanguageHint => (
        "切换后立即生效，并写入配置文件。",
        "Takes effect immediately and is saved to the config file."
    ),
    AboutClean => ("构建状态：干净（与提交一致）", "Build state: clean (matches the commit)"),
    AboutDirty => (
        "构建状态：包含未提交改动",
        "Build state: contains uncommitted changes"
    ),
    AboutUnknown => ("构建状态：未知", "Build state: unknown"),
    AboutTechnical => ("技术详情（反馈问题时提供）", "Technical details (include when reporting a problem)"),
    AboutGitCommit => ("Git 提交：{}", "Git commit: {}"),
    AboutBuildId => ("构建 ID：{}", "Build ID: {}"),
    AboutLinks => ("相关网站", "Links"),
    AboutProfiles => ("支持的屏幕预设", "Supported screen presets"),
    AboutDeps => ("第三方依赖", "Third-party dependencies"),
    AboutAdvice => (
        "使用建议：先做环境诊断，再运行试运行；正式运行需要明确确认。",
        "Recommended: run the environment check, then a dry run; a live run always needs explicit confirmation."
    ),
    AboutLdplayerLink => ("雷电模拟器官网", "LDPlayer website"),
    AboutAppLink => ("智汇福大官网", "Zhihui Fuda website"),
    DepImage => (
        "image：PNG/JPEG 解码和轻量图像处理",
        "image: PNG/JPEG decoding and light image processing"
    ),
    DepGui => ("eframe/egui：原生界面", "eframe/egui: native GUI"),
    DepRfd => ("rfd：安装包文件选择器", "rfd: package file picker"),
    DepSerde => (
        "serde / serde_json / toml：配置与结构化日志",
        "serde / serde_json / toml: config and structured logs"
    ),
    DepExternal => (
        "ADB 与 LDPlayer ldconsole.exe：外部设备控制",
        "ADB and LDPlayer ldconsole.exe: external device control"
    ),

    // ── Automatic-login result lines ───────────────────────────────────────
    AutoLoginDone => ("已执行一键登录，并确认离开登录页", "One-click login performed and login page confirmed gone"),
    AutoLoginUnconfirmed => ("已执行一键登录，但未确认成功", "One-click login performed, success not confirmed"),
    AutoLoginUnknown => ("已执行一键登录，结果未知", "One-click login performed, outcome unknown"),
    AutoLoginDryReady => ("试运行：登录页条件满足，未点击", "Dry run: login conditions met, nothing tapped"),

    // ── Error explanations (machine code -> operator language) ─────────────
    ErrUnknownState => (
        "视觉状态无法确认；请查看最新日志截图。",
        "The visual state could not be confirmed; check the latest log screenshots."
    ),
    ErrOverlay => (
        "有非目标 App 的窗口（弹窗或广告）遮挡页面；请手动关闭后重新运行，程序没有点击。",
        "A window from another app (pop-up or ad) covers the page; close it manually and retry. Nothing was tapped."
    ),
    ErrStaleRefreshFailed => (
        "启动时停在旧签到页，返回后没有回到服务页；请手动返回首页。",
        "The run started on a stale check-in page and did not return to the services page; go back to home manually."
    ),
    ErrStaleRefreshUnconfirmed => (
        "启动时停在旧签到页，返回后页面无法读取；请手动返回首页。",
        "The run started on a stale check-in page and the page became unreadable; go back to home manually."
    ),
    ErrLocationTimeout => (
        "定位等待达到上限；程序保留了最后一轮证据并安全停止，没有点击。",
        "Location wait reached its limit; the program kept the last round of evidence and stopped without tapping."
    ),
    ErrSharedPreflight => (
        "公共依赖（ldconsole.exe 或 adb.exe）不可用；本批次没有启动任何设备。",
        "A shared dependency (ldconsole.exe or adb.exe) is unavailable; no device was started in this batch."
    ),
    ErrRuntime => (
        "运行时出现未分类错误；请查看诊断报告和最新截图。",
        "An unclassified runtime error occurred; check the diagnostic report and latest screenshots."
    ),
    ErrSuccessTimeout => (
        "点击后没有在超时内确认成功；请人工核对页面。",
        "Success was not confirmed within the timeout after tapping; verify the page manually."
    ),
    ErrOutsideWindow => (
        "当前不在配置的签到时间窗口；没有点击。",
        "The current time is outside the configured check-in window; nothing was tapped."
    ),
    ErrForegroundChanged => (
        "目标 App 已离开前台（可能退到了桌面）；安全策略阻止继续操作，没有点击。请在模拟器里把 App 的普通签到页切回前台后重新运行。",
        "The target app left the foreground (perhaps it dropped to the launcher); the safety policy stopped the run without tapping. Bring the ordinary check-in page back to the foreground and retry."
    ),
    ErrForegroundQuery => (
        "无法读取前台应用；安全策略阻止继续操作。",
        "The foreground app could not be read; the safety policy stopped the run."
    ),
    ErrRouteCandidate => (
        "无法确认签到页面。请在模拟器中核对是否显示“无晚点名清单”或页面尚未加载。程序已停止，未点击签到。",
        "Could not confirm the check-in page. Check whether the emulator shows no evening check-in list or the page is still loading. The run stopped without tapping check-in."
    ),
    ErrUnsupportedMode => (
        "已确认在签到页，但类型不是普通晚点名（外勤或其他）；没有点击。",
        "The check-in page was confirmed but the type is not an ordinary evening check-in (field work or other); nothing was tapped."
    ),
    ErrEvidenceConflict => (
        "截图与页面结构证据冲突；安全策略阻止继续操作。",
        "Screenshot and page-structure evidence conflict; the safety policy stopped the run."
    ),
    ErrReadyConfirmation => (
        "点击前的二次就绪确认失败；没有点击。",
        "The second readiness check before tapping failed; nothing was tapped."
    ),
    ErrUnknownProfile => (
        "设备分辨率与 DPI 没有精确匹配的屏幕预设；没有点击。",
        "No screen preset exactly matches the device resolution and DPI; nothing was tapped."
    ),
    ErrUncalibratedProfile => (
        "当前屏幕预设还没有完成真机标定；没有点击。",
        "The current screen preset has not been calibrated on a real device; nothing was tapped."
    ),
    ErrLiveConfirmation => (
        "正式运行需要同时指定 live 与确认；没有点击。",
        "A live run needs both the live flag and explicit confirmation; nothing was tapped."
    ),
    ErrFieldWorkLive => (
        "外勤正式运行默认关闭；请先在配置文件中开启外勤授权，再重新明确确认。",
        "Live field work is off by default; enable the field-work permission in the config file, then confirm again."
    ),
    ErrFieldWorkObservation => (
        "外勤页面已记录观察结果；请人工确认页面状态。",
        "The field-work page was observed and recorded; confirm its state manually."
    ),
    ErrClickAuthorization => (
        "点击安全检查未满足；程序没有继续点击。",
        "The tap safety check was not satisfied; the program did not tap."
    ),
    ErrSuccessText => (
        "页面视觉状态显示成功，但页面文字没有确认；请人工核对。",
        "The visual state shows success but the page text did not confirm it; verify manually."
    ),
    ErrSuccessStale => (
        "成功页面显示的日期不是今天，不能算作本次签到成功。",
        "The success page shows a date that is not today, so this is not a successful check-in."
    ),
    ErrSuccessDateUnknown => (
        "页面没有提供可确认的成功日期；请人工核对。",
        "The page did not provide a confirmable success date; verify manually."
    ),
    ErrAppStart => (
        "目标 App 未能进入前台；请手动打开 App 并完成登录。",
        "The target app did not reach the foreground; open it manually and sign in."
    ),
    ErrAppRestartDialog => (
        "检测到 App 的“是否重启 App / 可能被界面劫持”安全弹窗；程序没有点击。请在模拟器里选择“取消”或“重启 App”，并检查悬浮窗、录屏或辅助功能后重试。",
        "The app's “restart app / possible screen hijacking” safety dialog is showing; the program did not tap it. Choose Cancel or Restart in the emulator, check for overlay, screen-recording, or accessibility tools, then retry."
    ),
    ErrLoginRequired => (
        "需要登录：请手动登录；或在配置中显式开启自动登录后重试（一键登录，默认关闭）。",
        "Sign-in required: sign in manually, or explicitly enable automatic login in the config and retry (one-click login, off by default)."
    ),
    ErrAutoLoginTerms => (
        "自动登录没有获得代勾选协议的授权；请开启协议授权，或手动登录。",
        "Automatic login is not authorized to tick the agreements; enable that consent or sign in manually."
    ),
    ErrAutoLoginConsent => (
        "无法确认登录页的协议复选框；程序没有点击登录，请手动登录。",
        "The login page's consent checkbox could not be confirmed; the program did not tap login. Sign in manually."
    ),
    ErrAutoLoginButton => (
        "登录页的“一键登录”按钮缺失或不唯一；程序没有点击，请手动登录。",
        "The one-click login button is missing or not unique; the program did not tap. Sign in manually."
    ),
    ErrAutoLoginAccount => (
        "登录页账号无法确认或与配置不符；程序没有点击登录。",
        "The login page account could not be confirmed or does not match the config; the program did not tap login."
    ),
    ErrAutoLoginBlocked => (
        "登录过程中出现外部窗口、弹窗，或 App 离开前台；程序已停止，没有继续点击。",
        "A foreign window or pop-up appeared, or the app left the foreground during login; the program stopped without further taps."
    ),
    ErrAutoLoginTimeout => (
        "一键登录后没有在超时内离开登录页；程序没有重试，请人工核对登录状态。",
        "The login page did not disappear within the timeout after one-click login; the program did not retry. Check the login state manually."
    ),
    ErrAutoLoginNotConfirmed => (
        "登录后页面证据不可读；程序已停止，没有继续签到。",
        "Page evidence became unreadable after login; the program stopped and did not continue."
    ),
    ErrInstanceNotListed => (
        "实例编号不存在，或实例已完全退出；请在 LDPlayer 中确认实例名，或先手动启动一次。程序没有点击。",
        "The instance index does not exist, or the instance has fully exited; confirm the instance in LDPlayer or start it manually once. Nothing was tapped."
    ),
    ErrInstanceNotRunning => (
        "实例当前未运行，且本命令不会自动启动实例；请先手动启动 LDPlayer 实例后重试。程序没有点击。",
        "The instance is not running and this command does not start it; start the LDPlayer instance manually and retry. Nothing was tapped."
    ),
    ErrInstanceLaunchTimeout => (
        "实例启动超时；请手动启动 LDPlayer 实例后重试。程序没有点击。",
        "The instance did not start within its budget; start it manually and retry. Nothing was tapped."
    ),
    ErrAdbConnectTimeout => (
        "ADB 没有就绪；请确认模拟器已启动并允许 ADB 调试。程序没有点击。",
        "ADB did not become ready; make sure the emulator is running and ADB debugging is allowed. Nothing was tapped."
    ),
    ErrBootTimeout => (
        "安卓系统没有在预算内完成引导；请手动启动实例并等桌面出现后重试。程序没有点击。",
        "Android did not finish booting within its budget; start the instance manually, wait for the home screen, and retry. Nothing was tapped."
    ),
    ErrNavigation => (
        "没有确认进入晚点名签到页；请手动导航后重试。",
        "The evening check-in page was not confirmed; navigate there manually and retry."
    ),
    ErrPageContextChanged => (
        "运行中页面或签到类型发生变化；安全策略已停止自动点击。",
        "The page or check-in type changed during the run; the safety policy stopped automatic tapping."
    ),
    ErrHierarchyUnavailable => (
        "无法读取页面结构；为安全起见没有继续点击。",
        "The page structure could not be read; for safety, no further taps were made."
    ),
    ErrAdbGeneric => (
        "ADB 设备不可用；请检查模拟器和 ADB 地址。",
        "The ADB device is unavailable; check the emulator and the ADB address."
    ),
    ErrGeneric => (
        "请查看诊断报告和运行日志。",
        "Check the diagnostic report and run log."
    ),
}

/// Look up a key in the requested language.
pub fn t(lang: Lang, key: Key) -> &'static str {
    let text = key.text();
    match lang {
        Lang::Zh => text.zh,
        Lang::En => text.en,
    }
}

pub const QUICK_USE_HEADINGS: [Key; 5] = [
    Key::QuickPrepareTitle,
    Key::QuickSetupTitle,
    Key::QuickRunTitle,
    Key::QuickIssuesTitle,
    Key::QuickAdvancedTitle,
];

pub const CONFIG_HEADINGS: [Key; 4] = [
    Key::ConfigSectionDeps,
    Key::ConfigSectionFieldWork,
    Key::ConfigSectionLogin,
    Key::ConfigSectionToml,
];

/// Terms introduced to a first-time user, with one-line meanings.
pub const TERM_HINTS: [TermHint; 9] = [
    TermHint {
        term: Key::TermInstance,
        meaning: Key::TermInstanceHint,
    },
    TermHint {
        term: Key::TermIndex,
        meaning: Key::TermIndexHint,
    },
    TermHint {
        term: Key::TermSerial,
        meaning: Key::TermSerialHint,
    },
    TermHint {
        term: Key::TermProfile,
        meaning: Key::TermProfileHint,
    },
    TermHint {
        term: Key::TermPage,
        meaning: Key::TermPageHint,
    },
    TermHint {
        term: Key::TermCheckinType,
        meaning: Key::TermCheckinTypeHint,
    },
    TermHint {
        term: Key::TermCheckinState,
        meaning: Key::TermCheckinStateHint,
    },
    TermHint {
        term: Key::TermResultFile,
        meaning: Key::TermResultFileHint,
    },
    TermHint {
        term: Key::TermEvents,
        meaning: Key::TermEventsHint,
    },
];

/// Human label for a run's exit status.
pub fn exit_code_label(lang: Lang, code: ExitCode) -> &'static str {
    match (lang, code) {
        (Lang::Zh, ExitCode::Ok) => "成功",
        (Lang::Zh, ExitCode::BusinessFailure) => "业务未完成",
        (Lang::Zh, ExitCode::DeviceFailure) => "设备不可用",
        (Lang::Zh, ExitCode::VisionFailure) => "画面识别失败",
        (Lang::Zh, ExitCode::ConfigFailure) => "配置错误",
        (Lang::Zh, ExitCode::Timeout) => "等待超时",
        (Lang::En, ExitCode::Ok) => "success",
        (Lang::En, ExitCode::BusinessFailure) => "business failure",
        (Lang::En, ExitCode::DeviceFailure) => "device unavailable",
        (Lang::En, ExitCode::VisionFailure) => "screen recognition failed",
        (Lang::En, ExitCode::ConfigFailure) => "configuration error",
        (Lang::En, ExitCode::Timeout) => "timed out",
    }
}

/// The user-facing summary of a run, in plain language.
pub fn run_result_label(lang: Lang, result: Option<&RunResult>) -> &'static str {
    let Some(result) = result else {
        return t(lang, Key::ResultNoRun);
    };
    if result.success {
        return t(lang, Key::ResultSuccess);
    }
    if result.field_work_ready {
        return t(lang, Key::ResultFieldWorkReady);
    }
    if result.dry_run_ready {
        return t(lang, Key::ResultDryReady);
    }
    if result.clicked {
        return t(lang, Key::ResultClickedUnconfirmed);
    }
    t(lang, Key::ResultIncomplete)
}

pub fn page_route_label(lang: Lang, route: Option<PageRoute>) -> &'static str {
    match route {
        Some(PageRoute::Login) => t(lang, Key::PageLogin),
        Some(PageRoute::Home) => t(lang, Key::PageHome),
        Some(PageRoute::Services) => t(lang, Key::PageServices),
        Some(PageRoute::Checkin) => t(lang, Key::PageCheckin),
        Some(PageRoute::RouteCandidateUnconfirmed) => t(lang, Key::PageCandidateUnconfirmed),
        Some(PageRoute::RouteEvidenceConflict) => t(lang, Key::PageEvidenceConflict),
        Some(PageRoute::StalePageRefreshFailed) => t(lang, Key::PageStaleRefreshFailed),
        Some(PageRoute::StalePageRefreshUnconfirmed) => t(lang, Key::PageStaleRefreshUnconfirmed),
        Some(PageRoute::AppRestartDialog) => t(lang, Key::PageAppRestartDialog),
        Some(PageRoute::AppLeftForeground) => t(lang, Key::PageAppLeftForeground),
        Some(PageRoute::Unknown) | None => t(lang, Key::CommonNotRead),
    }
}

pub fn checkin_mode_label(lang: Lang, mode: Option<CheckinMode>) -> &'static str {
    match mode {
        Some(CheckinMode::Ordinary) => t(lang, Key::ModeOrdinary),
        Some(CheckinMode::FieldWork) => t(lang, Key::ModeFieldWork),
        Some(CheckinMode::Unsupported) => t(lang, Key::ModeUnsupported),
        None => t(lang, Key::CommonNotRead),
    }
}

pub fn checkin_state_label(lang: Lang, state: Option<CheckinState>) -> &'static str {
    match state {
        Some(CheckinState::Gray) => t(lang, Key::CheckinGray),
        Some(CheckinState::Locating) => t(lang, Key::CheckinLocating),
        Some(CheckinState::Ready) => t(lang, Key::CheckinReady),
        Some(CheckinState::SuccessToday) => t(lang, Key::CheckinSuccessToday),
        Some(CheckinState::SuccessStale) => t(lang, Key::CheckinSuccessStale),
        Some(CheckinState::SuccessDateUnknown) => t(lang, Key::CheckinSuccessDateUnknown),
        Some(CheckinState::FieldWorkReady) => t(lang, Key::CheckinFieldWorkReady),
        Some(CheckinState::Unknown) | None => t(lang, Key::CheckinUnknown),
    }
}

pub fn button_state_label(lang: Lang, state: Option<&str>) -> &'static str {
    match state {
        Some("gray") => t(lang, Key::ButtonGray),
        Some("locating") => t(lang, Key::ButtonLocating),
        Some("ready") => t(lang, Key::ButtonReady),
        Some("success") => t(lang, Key::ButtonSuccess),
        _ => t(lang, Key::ButtonUnknown),
    }
}

pub fn location_state_label(lang: Lang, state: Option<&str>) -> &'static str {
    match state {
        Some("locating") => t(lang, Key::CheckinLocating),
        Some("ready") => t(lang, Key::LocationReady),
        Some("success") => t(lang, Key::LocationSuccess),
        _ => t(lang, Key::CheckinUnknown),
    }
}

pub fn hierarchy_status_label(lang: Lang, state: Option<&str>) -> &'static str {
    match state {
        Some("dump_succeeded") => t(lang, Key::HierarchySucceeded),
        Some("dump_failed") => t(lang, Key::HierarchyFailed),
        Some("dump_empty_or_truncated") => t(lang, Key::HierarchyEmpty),
        Some("route_markers_missing") => t(lang, Key::HierarchyMarkersMissing),
        _ => t(lang, Key::CommonNotRead),
    }
}

pub fn page_loading_label(lang: Lang, loading: Option<bool>) -> &'static str {
    match loading {
        Some(true) => t(lang, Key::LoadingYes),
        Some(false) => t(lang, Key::LoadingNo),
        None => t(lang, Key::CommonNotRead),
    }
}

/// Human label for a log entry kind; unknown kinds stay verbatim.
pub fn log_kind_label(lang: Lang, kind: &str) -> String {
    match kind {
        "run" => t(lang, Key::LogKindRun).to_string(),
        "sync" => t(lang, Key::LogKindSync).to_string(),
        "batch" => t(lang, Key::LogKindBatch).to_string(),
        other => other.to_string(),
    }
}

/// Human label for a log entry status; unknown statuses stay verbatim.
pub fn log_status_label(lang: Lang, status: &str) -> String {
    match status {
        "success" => t(lang, Key::LogStatusSuccess).to_string(),
        "dry_run_ready" => t(lang, Key::LogStatusReady).to_string(),
        "field_work_ready" => t(lang, Key::LogStatusFieldReady).to_string(),
        "blocked" => t(lang, Key::LogStatusBlocked).to_string(),
        "failed" => t(lang, Key::LogStatusFailed).to_string(),
        "incomplete" => t(lang, Key::LogStatusIncomplete).to_string(),
        other => other.to_string(),
    }
}

pub fn refresh_label(lang: Lang, attempted: Option<bool>, confirmed: Option<bool>) -> String {
    let attempted = match attempted {
        Some(true) => t(lang, Key::RefreshAttempted),
        Some(false) => t(lang, Key::RefreshNotNeeded),
        None => t(lang, Key::CommonNotRead),
    };
    let confirmed = match confirmed {
        Some(true) => t(lang, Key::RefreshConfirmed),
        Some(false) => t(lang, Key::RefreshUnconfirmed),
        None => t(lang, Key::CommonNotRead),
    };
    format!("{attempted} / {confirmed}")
}

/// Plain-language explanation for a stable machine error code.
pub fn explain_error(lang: Lang, code: &str) -> &'static str {
    let key = match code {
        "unknown_state" => Key::ErrUnknownState,
        "overlay_present" => Key::ErrOverlay,
        "stale_page_refresh_failed" => Key::ErrStaleRefreshFailed,
        "stale_page_refresh_unconfirmed" => Key::ErrStaleRefreshUnconfirmed,
        "location_timeout" | "location_wait_timeout" => Key::ErrLocationTimeout,
        "shared_preflight_failed" => Key::ErrSharedPreflight,
        "runtime_error" => Key::ErrRuntime,
        "success_timeout" => Key::ErrSuccessTimeout,
        "outside_window" => Key::ErrOutsideWindow,
        "foreground_package_changed" | "target_app_not_foreground" => Key::ErrForegroundChanged,
        "foreground_query_failed" => Key::ErrForegroundQuery,
        "route_candidate_unconfirmed" => Key::ErrRouteCandidate,
        "unsupported_checkin_mode" => Key::ErrUnsupportedMode,
        "route_evidence_conflict" => Key::ErrEvidenceConflict,
        "ready_confirmation_failed" => Key::ErrReadyConfirmation,
        "unknown_profile" => Key::ErrUnknownProfile,
        "uncalibrated_profile" => Key::ErrUncalibratedProfile,
        "live_confirmation_required" => Key::ErrLiveConfirmation,
        "field_work_live_unsupported" => Key::ErrFieldWorkLive,
        "field_work_observation_only" => Key::ErrFieldWorkObservation,
        "click_authorization_failed" => Key::ErrClickAuthorization,
        "success_text_unconfirmed" => Key::ErrSuccessText,
        "success_stale" => Key::ErrSuccessStale,
        "success_date_unknown" => Key::ErrSuccessDateUnknown,
        "app_start_timeout" => Key::ErrAppStart,
        "app_restart_dialog" => Key::ErrAppRestartDialog,
        "login_required" => Key::ErrLoginRequired,
        "auto_login_terms_not_accepted" => Key::ErrAutoLoginTerms,
        "auto_login_consent_not_found" | "auto_login_consent_unconfirmed" => {
            Key::ErrAutoLoginConsent
        }
        "auto_login_button_ambiguous" => Key::ErrAutoLoginButton,
        "auto_login_account_mismatch" | "auto_login_account_unknown" => Key::ErrAutoLoginAccount,
        "auto_login_blocked" => Key::ErrAutoLoginBlocked,
        "auto_login_timeout" => Key::ErrAutoLoginTimeout,
        "auto_login_not_confirmed" => Key::ErrAutoLoginNotConfirmed,
        "instance_not_listed" => Key::ErrInstanceNotListed,
        "instance_not_running" => Key::ErrInstanceNotRunning,
        "instance_launch_timeout" => Key::ErrInstanceLaunchTimeout,
        "adb_connect_timeout" => Key::ErrAdbConnectTimeout,
        "boot_timeout" => Key::ErrBootTimeout,
        "navigation_not_confirmed" => Key::ErrNavigation,
        "page_context_changed" => Key::ErrPageContextChanged,
        "hierarchy_unavailable" => Key::ErrHierarchyUnavailable,
        other if other.contains("ADB") => Key::ErrAdbGeneric,
        _ => Key::ErrGeneric,
    };
    t(lang, key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_has_non_empty_zh_and_en_text() {
        for key in Key::ALL {
            let zh = t(Lang::Zh, *key);
            let en = t(Lang::En, *key);
            assert!(!zh.trim().is_empty(), "empty zh text for {key:?}");
            assert!(!en.trim().is_empty(), "empty en text for {key:?}");
        }
    }

    #[test]
    fn chinese_copy_stays_chinese_and_english_copy_has_no_chinese() {
        fn has_cjk(value: &str) -> bool {
            value
                .chars()
                .any(|ch| matches!(ch, '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}'))
        }
        for key in Key::ALL {
            let zh = t(Lang::Zh, *key);
            let en = t(Lang::En, *key);
            assert!(
                has_cjk(zh),
                "zh text for {key:?} has no Chinese characters: {zh}"
            );
            assert!(
                !has_cjk(en),
                "en text for {key:?} contains Chinese characters: {en}"
            );
        }
        assert_eq!(language_name(Lang::Zh), "中文");
        assert_eq!(language_name(Lang::En), "English");
    }

    #[test]
    fn every_page_heading_has_bilingual_text() {
        for heading in QUICK_USE_HEADINGS.iter().chain(CONFIG_HEADINGS.iter()) {
            for lang in [Lang::Zh, Lang::En] {
                assert!(!t(lang, *heading).trim().is_empty());
            }
        }
    }

    #[test]
    fn every_term_has_a_meaning() {
        for term in TERM_HINTS {
            for lang in [Lang::Zh, Lang::En] {
                assert!(!t(lang, term.term).trim().is_empty());
                assert!(!t(lang, term.meaning).trim().is_empty());
            }
        }
    }

    #[test]
    fn language_parsing_is_explicit() {
        assert_eq!(Lang::from_config("zh"), Lang::Zh);
        assert_eq!(Lang::from_config("EN"), Lang::En);
        assert_eq!(Lang::from_config(" English "), Lang::En);
        assert_eq!(Lang::from_config(""), Lang::Zh);
        assert_eq!(Lang::Zh.code(), "zh");
        assert_eq!(Lang::En.code(), "en");
    }

    #[test]
    fn exit_codes_have_plain_language_labels() {
        for code in [
            ExitCode::Ok,
            ExitCode::BusinessFailure,
            ExitCode::DeviceFailure,
            ExitCode::VisionFailure,
            ExitCode::ConfigFailure,
            ExitCode::Timeout,
        ] {
            let zh = exit_code_label(Lang::Zh, code);
            let en = exit_code_label(Lang::En, code);
            assert!(!zh.contains("Ok"), "{code:?} leaked an enum name: {zh}");
            assert!(
                !zh.contains("Failure"),
                "{code:?} leaked an enum name: {zh}"
            );
            assert!(!en.contains("Ok"), "{code:?} leaked an enum name: {en}");
            assert!(
                !en.contains("DeviceFailure"),
                "{code:?} leaked an enum name: {en}"
            );
        }
    }

    #[test]
    fn run_result_labels_cover_every_outcome() {
        let empty: RunResult = serde_json::from_value(serde_json::json!({
            "mode": "dry-run",
            "serial": "",
            "state_history": [],
            "clicked": false,
            "success": false,
            "dry_run_ready": false
        }))
        .expect("minimal run result must deserialize");
        assert_eq!(
            run_result_label(Lang::Zh, None),
            t(Lang::Zh, Key::ResultNoRun)
        );
        assert_eq!(
            run_result_label(Lang::Zh, Some(&empty)),
            t(Lang::Zh, Key::ResultIncomplete)
        );

        let mut success = empty.clone();
        success.success = true;
        assert_eq!(
            run_result_label(Lang::En, Some(&success)),
            t(Lang::En, Key::ResultSuccess)
        );

        let mut dry = empty.clone();
        dry.dry_run_ready = true;
        assert_eq!(
            run_result_label(Lang::Zh, Some(&dry)),
            t(Lang::Zh, Key::ResultDryReady)
        );

        let mut field = empty.clone();
        field.field_work_ready = true;
        assert_eq!(
            run_result_label(Lang::Zh, Some(&field)),
            t(Lang::Zh, Key::ResultFieldWorkReady)
        );

        let mut clicked = empty;
        clicked.clicked = true;
        assert_eq!(
            run_result_label(Lang::Zh, Some(&clicked)),
            t(Lang::Zh, Key::ResultClickedUnconfirmed)
        );
    }

    #[test]
    fn enum_labels_never_fall_back_to_machine_strings() {
        for route in [
            PageRoute::Login,
            PageRoute::Home,
            PageRoute::Services,
            PageRoute::Checkin,
            PageRoute::Unknown,
            PageRoute::RouteCandidateUnconfirmed,
            PageRoute::RouteEvidenceConflict,
            PageRoute::StalePageRefreshFailed,
            PageRoute::StalePageRefreshUnconfirmed,
            PageRoute::AppRestartDialog,
            PageRoute::AppLeftForeground,
        ] {
            for lang in [Lang::Zh, Lang::En] {
                let label = page_route_label(lang, Some(route));
                assert_ne!(label, route.as_str(), "route {route:?} leaked its code");
            }
        }
        for mode in [
            CheckinMode::Ordinary,
            CheckinMode::FieldWork,
            CheckinMode::Unsupported,
        ] {
            for lang in [Lang::Zh, Lang::En] {
                assert_ne!(checkin_mode_label(lang, Some(mode)), mode.as_str());
            }
        }
        for state in [
            CheckinState::Gray,
            CheckinState::Locating,
            CheckinState::Ready,
            CheckinState::SuccessToday,
            CheckinState::SuccessStale,
            CheckinState::SuccessDateUnknown,
            CheckinState::FieldWorkReady,
            CheckinState::Unknown,
        ] {
            for lang in [Lang::Zh, Lang::En] {
                assert_ne!(checkin_state_label(lang, Some(state)), state.as_str());
            }
        }
        for value in ["gray", "locating", "ready", "success", "unknown"] {
            for lang in [Lang::Zh, Lang::En] {
                assert_ne!(button_state_label(lang, Some(value)), value);
            }
        }
        for value in [
            "dump_succeeded",
            "dump_failed",
            "dump_empty_or_truncated",
            "route_markers_missing",
        ] {
            for lang in [Lang::Zh, Lang::En] {
                assert_ne!(hierarchy_status_label(lang, Some(value)), value);
            }
        }
        assert_eq!(
            page_route_label(Lang::Zh, None),
            t(Lang::Zh, Key::CommonNotRead)
        );
    }

    #[test]
    fn every_documented_error_code_has_a_specific_hint() {
        let codes = [
            "unknown_state",
            "overlay_present",
            "stale_page_refresh_failed",
            "stale_page_refresh_unconfirmed",
            "location_timeout",
            "shared_preflight_failed",
            "runtime_error",
            "success_timeout",
            "outside_window",
            "foreground_package_changed",
            "target_app_not_foreground",
            "route_candidate_unconfirmed",
            "unsupported_checkin_mode",
            "route_evidence_conflict",
            "ready_confirmation_failed",
            "unknown_profile",
            "uncalibrated_profile",
            "live_confirmation_required",
            "field_work_live_unsupported",
            "click_authorization_failed",
            "success_text_unconfirmed",
            "success_stale",
            "success_date_unknown",
            "app_start_timeout",
            "app_restart_dialog",
            "login_required",
            "auto_login_terms_not_accepted",
            "instance_not_listed",
            "instance_not_running",
            "instance_launch_timeout",
            "adb_connect_timeout",
            "boot_timeout",
            "navigation_not_confirmed",
            "page_context_changed",
            "hierarchy_unavailable",
        ];
        for code in codes {
            for lang in [Lang::Zh, Lang::En] {
                assert_ne!(
                    explain_error(lang, code),
                    explain_error(lang, "some_unmapped_code"),
                    "code {code} fell through to the generic hint"
                );
            }
        }
    }
}
