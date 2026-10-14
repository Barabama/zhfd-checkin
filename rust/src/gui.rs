use crate::config::ConfigStore;
use crate::domain::RunResult;
use crate::{
    ConfigCommand, InstanceTargetArgs, RunArgs, TaskCommand, analyze_image_file,
    dependency_diagnostics, diagnose, dump_ui_hierarchy, inspect_apk_metadata, install_apk,
    installed_app_version, list_instance_summaries, run, runtime_with_target, sync_profile,
    task_command, validate_dependency_selection,
};
use crate::{config, logger, profile};
use eframe::egui;
use std::sync::{Arc, Mutex};
use std::thread;

/// `startup_error` carries a config load/validation failure. When it is set the
/// GUI opens directly on the Repair page instead of failing to start, so a
/// damaged config never leaves the user without a recovery entry point.
pub fn launch(store: ConfigStore, startup_error: Option<String>) -> anyhow::Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "智汇福大自动签到",
        options,
        Box::new(move |cc| {
            configure_fonts(&cc.egui_ctx);
            Ok(Box::new(GuiApp::new(store, startup_error)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("GUI 启动失败: {}", e))
}

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "noto_sans_sc".into(),
        egui::FontData::from_static(include_bytes!("../assets/NotoSansSC-VF.ttf")).into(),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "noto_sans_sc".into());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "noto_sans_sc".into());
    ctx.set_fonts(fonts);
}

struct GuiApp {
    store: ConfigStore,
    tab: usize,
    status: String,
    dependency_report: Option<crate::DependencyDiagnostics>,
    selected_ldplayer_candidate: Option<std::path::PathBuf>,
    selected_adb_candidate: Option<std::path::PathBuf>,
    dependency_scan: Option<Arc<Mutex<Option<crate::DependencyDiagnostics>>>>,
    apk_path: String,
    job: Option<GuiJob>,
    pending_sync: Option<String>,
    pending_live_run: bool,
    pending_migration: Option<std::path::PathBuf>,
    latest_result: Option<RunResult>,
    latest_log_dir: Option<String>,
    report_path: Option<String>,
    pending_apk_install: Option<std::path::PathBuf>,
    pending_apk_metadata: Option<crate::apk::ApkMetadata>,
    installed_version: Option<String>,
    app_version_checked: bool,
    apk_install_result: Option<String>,
    recent_logs: Vec<logger::LogSummary>,
    recent_events: Vec<logger::EventSummary>,
    logs_filter: String,
    selected_log_directory: Option<std::path::PathBuf>,
    selected_log_artifact: Option<std::path::PathBuf>,
    selected_log_preview: Option<String>,
    task_name: String,
    task_live: bool,
    task_all_instances: bool,
    run_all_instances: bool,
    instance_summaries: Vec<crate::InstanceSummary>,
    selected_instance_index: u32,
    /// Setup/Repair is the page shown while the configuration is missing or
    /// damaged; `setup_error` carries the load error text when there is one.
    setup_required: bool,
    setup_ldconsole: String,
    setup_adb: String,
    /// A genuine config load/validation failure. Only this makes the page a
    /// Repair condition.
    setup_error: Option<String>,
    /// Success or neutral feedback for the last Setup/Repair action. Rendered
    /// as ordinary text, never as a red "配置问题".
    setup_notice: Option<String>,
    /// Result slot for the background dependency validation. The validation
    /// spawns ldconsole/adb, so it must never run on the egui update thread.
    setup_validation: Option<SetupValidationSlot>,
    migration_target: String,
    /// Third click confirms "restore safe defaults"; the first two only arm it.
    confirm_reset_defaults: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum JobKind {
    General,
    AppInstall,
    AppVersion,
}

struct JobOutput {
    message: String,
    installed_version: Option<String>,
}

/// Shared slot the validation thread writes its outcome into; the UI thread
/// polls it each frame with `try_lock` so a hanging child process cannot block
/// rendering.
type SetupValidationSlot = Arc<Mutex<Option<Result<String, String>>>>;

struct GuiJob {
    label: String,
    kind: JobKind,
    result: Arc<Mutex<Option<JobOutput>>>,
}

fn run_target_for_selection(
    all_instances: bool,
    selected_instance_index: u32,
) -> InstanceTargetArgs {
    if all_instances {
        InstanceTargetArgs::default()
    } else {
        InstanceTargetArgs {
            instance_index: Some(selected_instance_index),
            serial: None,
        }
    }
}

impl GuiApp {
    fn new(store: ConfigStore, startup_error: Option<String>) -> Self {
        let apk_path = store.config.app.apk_path.clone();
        let selected_instance_index = store
            .config
            .emulator
            .effective_instances()
            .first()
            .map(|instance| instance.instance_index)
            .unwrap_or(store.config.emulator.instance_index);
        // A missing dependency path or a created-default config means the user
        // has not finished first-run setup yet, so open on the Setup page.
        let setup_required = startup_error.is_some()
            || store.created_default
            || store.config.emulator.ldconsole_path.trim().is_empty()
            || store.config.emulator.adb_path.trim().is_empty();
        let setup_ldconsole = store.config.emulator.ldconsole_path.clone();
        let setup_adb = store.config.emulator.adb_path.clone();
        let setup_error = startup_error.or_else(|| store.recovery_error.clone());
        let migration_target = config::recommended_local_app_data_root()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        Self {
            store,
            tab: if setup_required { SETUP_TAB } else { 0 },
            status: if setup_required {
                "需要完成首次配置或修复配置后继续。".into()
            } else {
                "就绪。默认模式为 dry-run。".into()
            },
            dependency_report: None,
            selected_ldplayer_candidate: None,
            selected_adb_candidate: None,
            dependency_scan: None,
            apk_path,
            job: None,
            pending_sync: None,
            pending_live_run: false,
            pending_migration: None,
            latest_result: None,
            latest_log_dir: None,
            report_path: None,
            pending_apk_install: None,
            pending_apk_metadata: None,
            installed_version: None,
            app_version_checked: false,
            apk_install_result: None,
            recent_logs: Vec::new(),
            recent_events: Vec::new(),
            logs_filter: String::new(),
            selected_log_directory: None,
            selected_log_artifact: None,
            selected_log_preview: None,
            task_name: "ZHFD-AutoCheckin".into(),
            task_live: false,
            task_all_instances: false,
            run_all_instances: false,
            instance_summaries: Vec::new(),
            selected_instance_index,
            setup_required,
            setup_ldconsole,
            setup_adb,
            setup_error,
            setup_notice: None,
            setup_validation: None,
            migration_target,
            confirm_reset_defaults: false,
        }
    }

    fn start_background_job<F>(&mut self, label: impl Into<String>, task: F) -> bool
    where
        F: FnOnce() -> String + Send + 'static,
    {
        self.start_background_job_with_kind(label, JobKind::General, move || (task(), None))
    }

    fn start_background_job_with_kind<F>(
        &mut self,
        label: impl Into<String>,
        kind: JobKind,
        task: F,
    ) -> bool
    where
        F: FnOnce() -> (String, Option<String>) + Send + 'static,
    {
        if self.job.is_some() {
            self.status = "已有任务运行中".into();
            return false;
        }
        let label = label.into();
        let slot = Arc::new(Mutex::new(None));
        let result_slot = slot.clone();
        thread::spawn(move || {
            let (message, installed_version) = task();
            *result_slot.lock().unwrap() = Some(JobOutput {
                message,
                installed_version,
            });
        });
        self.status = format!("{} 已启动，正在后台运行……", label);
        self.job = Some(GuiJob {
            label,
            kind,
            result: slot,
        });
        true
    }

    fn start_dependency_scan(&mut self) {
        if self.dependency_scan.is_some() {
            self.status = "依赖扫描正在运行".into();
            return;
        }
        self.dependency_report = None;
        self.selected_ldplayer_candidate = None;
        self.selected_adb_candidate = None;
        let store = self.store.clone();
        let slot = Arc::new(Mutex::new(None));
        let result_slot = Arc::clone(&slot);
        thread::spawn(move || {
            let report = dependency_diagnostics(&store);
            if let Ok(mut result) = result_slot.lock() {
                *result = Some(report);
            }
        });
        self.dependency_scan = Some(slot);
        self.status = "正在扫描 LDPlayer / ADB 依赖……".into();
    }

    fn instance_target(&self) -> InstanceTargetArgs {
        InstanceTargetArgs {
            instance_index: Some(self.selected_instance_index),
            serial: None,
        }
    }

    fn instance_options(&self) -> Vec<(u32, String)> {
        self.store
            .config
            .emulator
            .effective_instances()
            .into_iter()
            .map(|instance| {
                (
                    instance.instance_index,
                    if instance.name.trim().is_empty() {
                        format!("实例 {}", instance.instance_index)
                    } else {
                        format!("{} ({})", instance.name, instance.instance_index)
                    },
                )
            })
            .collect()
    }

    fn start_run(&mut self, live: bool) {
        let store = self.store.clone();
        let all_instances = self.run_all_instances;
        // All-instances mode must not carry a single-instance selector.
        let target = run_target_for_selection(all_instances, self.selected_instance_index);
        let label = if live { "live 运行" } else { "dry-run" };
        self.start_background_job(label, move || {
            match run(
                &store,
                RunArgs {
                    dry_run: !live,
                    live,
                    confirm: live,
                    all_instances,
                    target,
                },
            ) {
                Ok(code) => format!("完成，退出码 {:?}", code),
                Err(error) => format!("失败: {:#}", error),
            }
        });
    }

    fn start_dry_run(&mut self) {
        self.start_run(false);
    }

    fn refresh_logs(&mut self) {
        match logger::recent_logs(&self.store.root, 100) {
            Ok(logs) => {
                self.recent_logs = logs;
                let selected_is_present =
                    self.selected_log_directory.as_ref().is_some_and(|path| {
                        self.recent_logs
                            .iter()
                            .any(|entry| &entry.directory == path)
                    });
                if !selected_is_present {
                    self.selected_log_directory = self
                        .recent_logs
                        .first()
                        .map(|entry| entry.directory.clone());
                }
                self.load_selected_log_artifact();
            }
            Err(error) => self.status = format!("读取运行日志失败: {:#}", error),
        }
    }

    fn select_log_directory(&mut self, directory: std::path::PathBuf) {
        self.selected_log_directory = Some(directory);
        self.load_selected_log_artifact();
    }

    /// Read the structured events of the selected run so the timeline shows
    /// foreground snapshots, hierarchy observations, route decisions and
    /// navigation actions from the same round (requirement FR-012).
    fn refresh_events(&mut self) {
        self.recent_events = match self.selected_log_directory.as_ref() {
            Some(directory) => logger::read_recent_events(directory, 200).unwrap_or_default(),
            None => Vec::new(),
        };
    }

    /// Validate the dependency paths off the UI thread.
    ///
    /// `validate_dependency_selection` runs `where.exe`, `ldconsole list2` and
    /// `adb version` with no timeout, so calling it inline in `update()` would
    /// freeze the window whenever one of those tools hangs.
    fn start_setup_validation(&mut self) {
        if self.setup_validation.is_some() {
            self.status = "依赖验证正在运行".into();
            return;
        }
        let store = self.store.clone();
        let ldconsole = self.setup_ldconsole.clone();
        let adb = self.setup_adb.clone();
        let slot = Arc::new(Mutex::new(None));
        let result_slot = Arc::clone(&slot);
        thread::spawn(move || {
            let outcome = validate_dependency_selection(&store, &ldconsole, &adb)
                .map_err(|error| format!("{error:#}"));
            if let Ok(mut result) = result_slot.lock() {
                *result = Some(outcome);
            }
        });
        self.setup_validation = Some(slot);
        self.status = "正在验证 LDPlayer / ADB 并读取实例……".into();
    }

    fn poll_setup_validation(&mut self) {
        let outcome = self
            .setup_validation
            .as_ref()
            .and_then(|slot| slot.try_lock().ok().and_then(|mut result| result.take()));
        let Some(outcome) = outcome else {
            return;
        };
        self.setup_validation = None;
        match outcome {
            Ok(summary) => {
                self.setup_error = None;
                self.setup_notice =
                    Some(format!("{summary}（已验证工具并读取到实例；尚未写入配置）"));
                self.status = "依赖验证通过".into();
            }
            Err(error) => {
                self.setup_error = Some(format!("验证失败：{error}"));
                self.setup_notice = None;
                self.status = "依赖验证失败".into();
            }
        }
    }

    fn migrate_data_root(&mut self, destination: std::path::PathBuf) {
        match self.store.migrate_to_root(&destination) {
            Ok(store) => {
                self.store = store;
                self.apk_path = self.store.config.app.apk_path.clone();
                self.setup_ldconsole = self.store.config.emulator.ldconsole_path.clone();
                self.setup_adb = self.store.config.emulator.adb_path.clone();
                self.setup_notice = Some(format!(
                    "数据目录已迁移到 {}；源目录保留未删除。",
                    self.store.root.display()
                ));
                self.status = "数据目录迁移完成".into();
                self.refresh_latest_result();
            }
            Err(error) => {
                // A failed migration must leave the store on the original root.
                self.setup_error = Some(format!(
                    "迁移失败，仍使用原数据目录 {}：{error:#}",
                    self.store.root.display()
                ));
            }
        }
    }

    fn load_selected_log_artifact(&mut self) {
        let Some(directory) = self.selected_log_directory.as_ref() else {
            self.selected_log_artifact = None;
            self.selected_log_preview = None;
            return;
        };
        match logger::read_log_artifact(directory) {
            Ok((path, text)) => {
                self.selected_log_artifact = Some(path);
                self.selected_log_preview = Some(text);
            }
            Err(error) => {
                self.selected_log_artifact = None;
                self.selected_log_preview = None;
                self.status = format!("读取选中日志失败: {error:#}");
            }
        }
        self.refresh_events();
    }

    fn refresh_latest_result(&mut self) {
        match logger::latest_result(&self.store.root) {
            Ok(Some((dir, result))) => {
                self.latest_log_dir = Some(dir.display().to_string());
                self.latest_result = Some(result);
                self.refresh_logs();
            }
            Ok(None) => {
                self.latest_log_dir = None;
                self.latest_result = None;
                self.refresh_logs();
            }
            Err(error) => self.status = format!("读取运行结果失败: {:#}", error),
        }
    }

    fn poll_job(&mut self) {
        let result = self
            .job
            .as_ref()
            .and_then(|job| job.result.lock().ok().and_then(|mut guard| guard.take()));
        if let Some(result) = result {
            let (label, kind) = self
                .job
                .as_ref()
                .map(|job| (job.label.clone(), job.kind))
                .unwrap_or_else(|| ("后台任务".into(), JobKind::General));
            if let Some(version) = result.installed_version {
                self.installed_version = Some(version);
            }
            if kind == JobKind::AppInstall {
                self.apk_install_result = Some(result.message.clone());
            }
            self.status = format!("{}：{}", label, result.message);
            self.job = None;
            self.refresh_latest_result();
            self.refresh_logs();
        }
    }

    fn start_app_version_query(&mut self) {
        self.app_version_checked = true;
        let store = self.store.clone();
        let started =
            self.start_background_job_with_kind("读取已安装版本", JobKind::AppVersion, {
                let target = self.instance_target();
                move || match installed_app_version(&store, &target) {
                    Ok(version) => (format!("当前已安装版本：{}", version), Some(version)),
                    Err(error) => (format!("失败: {:#}", error), None),
                }
            });
        if !started {
            self.app_version_checked = false;
        }
    }

    fn export_report(&mut self) {
        match logger::write_diagnostic_report(&self.store) {
            Ok(path) => {
                self.report_path = Some(path.display().to_string());
                self.status = format!("诊断报告已导出: {}", path.display());
            }
            Err(error) => self.status = format!("诊断报告导出失败: {:#}", error),
        }
    }

    fn save_apk_path(&mut self, path: std::path::PathBuf) {
        self.apk_path = config::path_for_config(&self.store.root, &path);
        self.store.config.app.apk_path = self.apk_path.clone();
        match self.store.save() {
            Ok(()) => self.status = format!("已保存 APK 路径: {}", self.apk_path),
            Err(e) => self.status = format!("保存配置失败: {:#}", e),
        }
    }
}

fn ordered_profiles() -> Vec<&'static profile::Profile> {
    let mut profiles: Vec<_> = profile::PROFILES.iter().collect();
    profiles.sort_by(|left, right| {
        right
            .calibrated
            .cmp(&left.calibrated)
            .then_with(|| left.orientation.as_str().cmp(right.orientation.as_str()))
            .then_with(|| left.width.cmp(&right.width))
            .then_with(|| left.height.cmp(&right.height))
            .then_with(|| left.density_dpi.cmp(&right.density_dpi))
            .then_with(|| left.id.cmp(right.id))
    });
    profiles
}

fn run_result_status(result: &RunResult) -> String {
    if result.success {
        "success_today".into()
    } else if result.field_work_ready {
        "field_work_ready (dry-run only)".into()
    } else if result.dry_run_ready {
        "dry_run_ready".into()
    } else {
        result.error.clone().unwrap_or_else(|| "incomplete".into())
    }
}

fn explain_error(error: &str) -> &'static str {
    match error {
        "unknown_state" => "视觉状态未能确认；请查看最新日志截图。",
        "overlay_present" => {
            "检测到非目标应用的窗口（弹窗/广告）遮挡了页面；请手动关闭后重新运行，程序未点击。"
        }
        "stale_page_refresh_failed" => "启动时停留在旧签到页且返回后未回到服务页；请手动返回首页。",
        "stale_page_refresh_unconfirmed" => {
            "启动时停留在旧签到页且返回后页面无法读取；请手动返回首页。"
        }
        "location_timeout" | "location_wait_timeout" => {
            "定位等待达到上限；已保留最后一轮证据并安全停止，未执行点击。"
        }
        "shared_preflight_failed" => {
            "公共依赖(ldconsole.exe / adb.exe)不可用；本批次没有启动任何设备。"
        }
        "runtime_error" => "运行时出现未分类错误；请查看诊断报告和最新截图。",
        "success_timeout" => "点击后未在超时内确认成功；请人工核对页面。",
        "outside_window" => "当前不在配置的签到时间窗口；未执行点击。",
        "foreground_package_changed" | "target_app_not_foreground" => {
            "目标 App 已离开前台（可能退到了桌面）；安全策略阻止继续操作，程序未点击。请在模拟器中手动把 App 的普通签到页切回前台后重新运行。"
        }
        "foreground_query_failed" => "无法读取前台应用；安全策略阻止继续操作。",
        "route_candidate_unconfirmed" => "页面截图仅能形成候选，层级证据未确认；未执行点击。",
        "unsupported_checkin_mode" => {
            "已确认在签到页，但签到模式不是普通晚点名（外勤或其他）；未执行点击。"
        }
        "route_evidence_conflict" => "页面截图与层级证据冲突；安全策略阻止继续操作。",
        "ready_confirmation_failed" => "点击前二次 ready 确认失败；未执行点击。",
        "unknown_profile" => "设备分辨率/DPI 没有精确 profile；未执行点击。",
        "uncalibrated_profile" => "当前 profile 尚未完成生产标定；未执行点击。",
        "live_confirmation_required" => "正式模式需要同时指定 --live --confirm；未执行点击。",
        "field_work_live_unsupported" => "外勤签到目前仅支持 dry-run 观察；请在模拟器中人工完成。",
        "field_work_observation_only" => "外勤页面已记录观察结果；请人工确认页面状态。",
        "click_authorization_failed" => "点击安全门禁未满足；程序没有继续点击。",
        "success_text_unconfirmed" => "页面视觉状态为成功，但层级文字未确认；请人工核对。",
        "success_stale" => "成功页面显示的日期不是今天；不能视为本次签到成功。",
        "success_date_unknown" => "页面未提供可确认的成功日期；请人工核对。",
        "app_start_timeout" => "目标 App 未能进入前台；请手动打开 App 并完成登录。",
        "login_required" => "需要手动登录；程序不会输入凭据或自动点击登录。",
        "navigation_not_confirmed" => "未能确认进入晚点名签到页；请手动导航后重试。",
        "page_context_changed" => "运行中页面/签到模式发生变化；安全策略已停止自动点击。",
        "hierarchy_unavailable" => "无法读取页面结构；为安全起见没有继续点击。",
        other if other.contains("ADB") => "ADB 设备不可用；请检查模拟器和 serial。",
        _ => "请查看诊断报告和运行日志。",
    }
}

impl eframe::App for GuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_job();
        self.poll_setup_validation();
        let completed_dependency_scan = self
            .dependency_scan
            .as_ref()
            .and_then(|slot| slot.try_lock().ok().and_then(|mut result| result.take()));
        if let Some(report) = completed_dependency_scan {
            self.selected_ldplayer_candidate = report.ldplayer.selected_path();
            self.selected_adb_candidate = report.adb.selected_path();
            self.status = format!("{}\n{}", report.ldplayer.summary(), report.adb.summary());
            self.dependency_report = Some(report);
            self.dependency_scan = None;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        egui::SidePanel::left("navigation")
            .resizable(true)
            .default_width(178.0)
            .min_width(142.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.heading("签到助手");
                        ui.label(format!("v{}", env!("CARGO_PKG_VERSION")));
                        ui.separator();
                        ui.label(egui::RichText::new("工作台").strong());
                        for (index, label) in [(0, "运行控制"), (1, "环境诊断"), (2, "模拟器实例")]
                        {
                            if ui.selectable_label(self.tab == index, label).clicked() {
                                self.tab = index;
                            }
                        }
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new("业务配置").strong());
                        for (index, label) in [(3, "Profile"), (4, "应用 / APK"), (5, "配置文件")]
                        {
                            if ui.selectable_label(self.tab == index, label).clicked() {
                                self.tab = index;
                            }
                        }
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new("任务与证据").strong());
                        for (index, label) in [(6, "计划任务"), (7, "运行日志与结果")] {
                            if ui.selectable_label(self.tab == index, label).clicked() {
                                self.tab = index;
                                if index == 7 && self.recent_logs.is_empty() {
                                    self.refresh_logs();
                                }
                            }
                        }
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new("帮助").strong());
                        if ui
                            .selectable_label(self.tab == HELP_TAB, "快速使用")
                            .clicked()
                        {
                            self.tab = HELP_TAB;
                        }
                        if ui.selectable_label(self.tab == 8, "About / 版本").clicked() {
                            self.tab = 8;
                        }
                        // First-run/Repair stays reachable from the normal
                        // workspace after the initial wizard has been left.
                        if ui
                            .selectable_label(self.tab == SETUP_TAB, "首次设置 / 配置修复")
                            .clicked()
                        {
                            self.tab = SETUP_TAB;
                        }
                    });
            });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.label(&self.status);
            if let Some(job) = &self.job {
                ui.label(format!("后台任务：{}（运行中）", job.label));
            }
        });
        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            0 => self.home(ui),
            1 => self.diagnostics(ui),
            2 => self.instances_page(ui),
            3 => self.profiles(ui),
            4 => self.app_page(ui),
            5 => self.config_page(ui),
            6 => self.tasks_page(ui),
            7 => self.logs_page(ui),
            8 => self.about(ui),
            HELP_TAB => self.quick_use(ui),
            _ => self.setup_page(ui),
        });
        self.sync_confirmation_window(ctx);
        self.apk_install_confirmation_window(ctx);
        self.live_confirmation_window(ctx);
        self.migration_confirmation_window(ctx);
    }
}

/// Navigation index of the Setup/Repair page.
const SETUP_TAB: usize = 9;
/// Navigation index for user-facing first-run and daily-use guidance.
const HELP_TAB: usize = 10;

impl GuiApp {
    /// A task-oriented guide for first-time and returning users. Keep each
    /// section actionable and link into the existing workbench pages instead
    /// of duplicating the configuration controls here.
    fn quick_use(&mut self, ui: &mut egui::Ui) {
        ui.heading("快速使用");
        ui.label("按顺序完成首次检查和 dry-run；长期使用时也可从这里进入诊断、日志和配置。");
        ui.separator();

        egui::CollapsingHeader::new("首次使用：准备环境并完成设置")
            .default_open(true)
            .show(ui, |ui| {
                ui.label("1. 启动 LDPlayer，并在模拟器中手动打开智汇福大 App、完成登录。程序不会替你输入凭据或同意协议。");
                ui.label("2. 在“首次设置 / 配置修复”中确认 ldconsole.exe、adb.exe 路径及实例；验证通过后保存。损坏的配置不会被静默覆盖。");
                ui.label("3. 到“环境诊断”检查所选实例、ADB 连接和设备几何/Profile；运行前请在模拟器中手动确认目标 App 位于前台。诊断只读取依赖和设备信息，不执行签到点击。");
                ui.label("4. 返回“运行控制”，选择一个实例后运行 dry-run。首次建议不要勾选全部实例。");
                ui.horizontal(|ui| {
                    if ui.button("打开设置 / 修复").clicked() {
                        self.tab = SETUP_TAB;
                    }
                    if ui.button("打开环境诊断").clicked() {
                        self.tab = 1;
                    }
                    if ui.button("返回运行控制").clicked() {
                        self.tab = 0;
                    }
                });
            });

        egui::CollapsingHeader::new("dry-run 结果怎么看")
            .default_open(true)
            .show(ui, |ui| {
                ui.label("dry-run 用于观察页面和签到状态，不执行签到点击；它不等于签到成功。即使顶层 status 显示 success，也要结合 success、dry_run_ready 和 error_code 判断。");
                ui.label("在“运行日志与结果”查看 status、route、checkin_mode、checkin_state 和 error_code；点击某次运行可查看事件时间线与文本结果，截图请通过“打开目录”在文件夹中查看。");
                ui.label("只有 result 明确显示 success=true 才代表程序验证了成功；dry_run_ready=true 仅表示观察到可签到状态。");
                if ui.button("打开运行日志与结果").clicked() {
                    self.tab = 7;
                    self.refresh_logs();
                }
            });

        egui::CollapsingHeader::new("常见情况：先看提示，再手动处理")
            .default_open(true)
            .show(ui, |ui| {
                ui.label("设备未连接 / ADB 失败：确认模拟器已启动、配置的实例与 serial 正确，再运行环境诊断。");
                ui.label("需要登录：在模拟器中手动登录后重新运行；程序不会自动登录。");
                ui.label("检测到弹窗或广告：按提示在模拟器中手动关闭，再重试。程序不会自动关闭、同意或点击弹窗。");
                ui.label("页面未确认、页面证据冲突或定位超时：停止并查看最新日志/截图；确认页面状态后再决定是否重试，不要反复盲目运行。");
                ui.label("配置损坏：进入“首次设置 / 配置修复”检查原文件并按需修复；原配置不会被 CLI 静默重置。");
                ui.horizontal(|ui| {
                    if ui.button("查看运行结果 / 日志").clicked() {
                        self.tab = 7;
                        self.refresh_logs();
                    }
                    if ui.button("重新运行环境诊断").clicked() {
                        self.tab = 1;
                    }
                    if ui.button("打开配置修复").clicked() {
                        self.tab = SETUP_TAB;
                    }
                });
            });

        egui::CollapsingHeader::new("长期使用：多实例、计划任务与数据")
            .default_open(false)
            .show(ui, |ui| {
                ui.label("确认单实例 dry-run 正常后，再按需选择全部已启用实例；批量运行会逐个记录结果，单实例失败不会抹掉其他实例结果。");
                ui.label("计划任务建议保持 dry-run。不要将 live 设置为无人值守任务；live 需要显式确认，并继续受页面、前台、时间窗、Profile 和 ready 状态门禁保护。");
                ui.label("运行日志、截图和报告保存在当前数据目录。导出或分享诊断资料前，请检查其中的本机路径、实例信息和截图。");
                ui.label("迁移数据目录前确认目标位置可写；迁移会先备份并验证，不会在目标配置已存在时覆盖它。");
                ui.horizontal(|ui| {
                    if ui.button("打开计划任务").clicked() {
                        self.tab = 6;
                    }
                    if ui.button("打开数据与配置").clicked() {
                        self.tab = 5;
                    }
                    if ui.button("查看日志和证据").clicked() {
                        self.tab = 7;
                        self.refresh_logs();
                    }
                });
            });

        ui.separator();
        ui.label("安全边界：未知页面、证据不足或不支持的签到类型会停止；dry-run 不点击。live 只对明确确认的普通签到流程开放，且不自动登录、不自动同意、不自动关闭广告。");
        if ui.button("打开安全策略说明").clicked() {
            self.tab = 8;
        }
    }

    /// Setup (first run) and Repair (damaged config) share one page: the same
    /// actions are needed in both cases, and the frozen decisions require a
    /// reachable recovery entry rather than a startup-failure dialog.
    fn setup_page(&mut self, ui: &mut egui::Ui) {
        // Only a genuine load/validation failure makes this a Repair page;
        // a successful reload or tool validation is ordinary feedback.
        let repairing = self.setup_error.is_some() || self.store.recovery_error.is_some();
        ui.heading(if repairing {
            "配置修复(Repair)"
        } else {
            "首次设置(Setup)"
        });
        ui.label(format!("配置文件:{}", self.store.path.display()));
        ui.label(format!("数据目录:{}", self.store.root.display()));
        ui.label(format!(
            "配置来源:{}",
            match self.store.origin {
                config::ConfigOrigin::Explicit => "显式 --config / 修复路径",
                config::ConfigOrigin::Portable => "EXE 便携目录",
                config::ConfigOrigin::LocalAppData => "%LOCALAPPDATA%\\ZHFDCheckin",
            }
        ));
        if let Some(error) = self.setup_error.clone() {
            ui.separator();
            ui.colored_label(egui::Color32::LIGHT_RED, format!("配置问题:{error}"));
            ui.label(
                "原文件未被覆盖。请重新载入、选择另一份配置，或在确认后显式恢复安全默认配置。",
            );
            if ui.button("复制错误信息").clicked() {
                ui.ctx().copy_text(error);
                self.status = "配置错误信息已复制".into();
            }
        }
        if let Some(notice) = self.setup_notice.clone() {
            ui.separator();
            ui.colored_label(egui::Color32::LIGHT_GREEN, format!("上次操作:{notice}"));
        }

        ui.separator();
        ui.label(egui::RichText::new("1. 依赖工具路径").strong());
        ui.horizontal(|ui| {
            ui.label("ldconsole.exe");
            ui.add(
                egui::TextEdit::singleline(&mut self.setup_ldconsole)
                    .desired_width(430.0)
                    .hint_text("雷电模拟器控制台可执行文件"),
            );
            if ui.button("浏览...").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("ldconsole.exe", &["exe"])
                    .pick_file()
            {
                self.setup_ldconsole = path.display().to_string();
                // D-02: the sibling adb.exe is a *candidate suggestion* only;
                // it is never written to the config without confirmation.
                let sibling = path.with_file_name("adb.exe");
                if sibling.is_file() {
                    self.setup_adb = sibling.display().to_string();
                    self.setup_notice = Some("已建议同目录 adb.exe；请确认后再保存。".into());
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("adb.exe");
            ui.add(
                egui::TextEdit::singleline(&mut self.setup_adb)
                    .desired_width(430.0)
                    .hint_text("Android Debug Bridge 可执行文件"),
            );
            if ui.button("浏览...").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("adb.exe", &["exe"])
                    .pick_file()
            {
                self.setup_adb = path.display().to_string();
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("验证工具并读取实例").clicked() {
                self.start_setup_validation();
            }
            if self.setup_validation.is_some() {
                ui.spinner();
                ui.label("验证中……");
            }
            if ui.button("确认并保存路径").clicked() {
                // Persist a candidate and only publish it after the atomic
                // write succeeded, so a failure keeps the last good config.
                let mut candidate = self.store.config.clone();
                candidate.emulator.ldconsole_path = self.setup_ldconsole.clone();
                candidate.emulator.adb_path = self.setup_adb.clone();
                match self.store.save_candidate(candidate) {
                    Ok(()) => {
                        self.setup_required = false;
                        self.setup_error = None;
                        self.setup_notice = None;
                        self.status = "依赖配置已保存".into();
                        self.tab = 0;
                    }
                    Err(error) => {
                        // The write is atomic, so both the on-disk config and the
                        // in-memory config are still the previous ones.
                        self.setup_error = Some(format!("保存失败,现有配置保持不变:{error:#}"));
                        self.setup_notice = None;
                    }
                }
            }
            if ui.button("打开配置所在目录").clicked()
                && let Some(parent) = self.store.path.parent()
                && let Err(error) = crate::process::open_path(parent)
            {
                self.status = format!("打开配置目录失败:{error:#}");
            }
        });

        ui.separator();
        ui.label(egui::RichText::new("2. 恢复与重新载入").strong());
        ui.horizontal_wrapped(|ui| {
            if ui.button("重新载入磁盘配置").clicked() {
                match self.store.reload() {
                    Ok(()) => {
                        self.setup_ldconsole = self.store.config.emulator.ldconsole_path.clone();
                        self.setup_adb = self.store.config.emulator.adb_path.clone();
                        self.setup_required = false;
                        self.setup_error = None;
                        self.setup_notice = Some("已从磁盘重新载入并验证配置。".into());
                    }
                    Err(error) => {
                        // Reload failure must keep the last valid in-memory config.
                        self.setup_error = Some(format!("未载入,保留当前有效配置:{error:#}"));
                        self.setup_notice = None;
                    }
                }
            }
            if ui.button("选择另一份 config.toml...").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("TOML", &["toml"])
                    .pick_file()
            {
                match ConfigStore::load_alternate(&path) {
                    Ok(store) => {
                        self.setup_ldconsole = store.config.emulator.ldconsole_path.clone();
                        self.setup_adb = store.config.emulator.adb_path.clone();
                        self.setup_required = store.created_default
                            || self.setup_ldconsole.trim().is_empty()
                            || self.setup_adb.trim().is_empty();
                        self.apk_path = store.config.app.apk_path.clone();
                        self.store = store;
                        self.setup_error = None;
                        self.setup_notice = Some("已切换到所选配置文件。".into());
                    }
                    Err(error) => {
                        self.setup_error = Some(format!("所选配置不可用:{error:#}"));
                        self.setup_notice = None;
                    }
                }
            }
            if ui
                .button(if self.confirm_reset_defaults {
                    "再次点击确认:备份并恢复安全默认"
                } else {
                    "恢复安全默认配置..."
                })
                .clicked()
            {
                if self.confirm_reset_defaults {
                    // `save_candidate` writes through `write_config`, which backs
                    // the existing file up before replacing it (decision D-06).
                    match self.store.save_candidate(config::Config::default()) {
                        Ok(()) => {
                            self.setup_ldconsole.clear();
                            self.setup_adb.clear();
                            self.setup_required = true;
                            self.setup_error = None;
                            self.setup_notice =
                                Some("已备份原配置并恢复安全默认值；请重新设置依赖路径。".into());
                        }
                        Err(error) => {
                            self.setup_error = Some(format!("恢复失败,原配置保持不变:{error:#}"));
                            self.setup_notice = None;
                        }
                    }
                    self.confirm_reset_defaults = false;
                } else {
                    self.confirm_reset_defaults = true;
                    self.status = "恢复默认配置需要再次确认。".into();
                }
            }
        });

        ui.separator();
        ui.label(egui::RichText::new("3. 数据目录迁移").strong());
        ui.label(
            "迁移会复制 config、logs、captures、reports、backups;仅在目标配置验证通过后切换当前数据目录,源目录不会删除。",
        );
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.migration_target)
                    .desired_width(430.0)
                    .hint_text("目标数据目录"),
            );
            if ui.button("选择目录...").clicked()
                && let Some(path) = rfd::FileDialog::new().pick_folder()
            {
                self.migration_target = path.display().to_string();
            }
            if ui.button("迁移并切换").clicked() {
                if self.migration_target.trim().is_empty() {
                    self.setup_notice = Some("请先选择目标数据目录。".into());
                } else {
                    self.pending_migration =
                        Some(std::path::PathBuf::from(self.migration_target.trim()));
                }
            }
        });

        ui.separator();
        ui.label(
            "程序不会自动登录、不会输入账号/密码/验证码,也不会自动签到。首次 dry-run 默认只运行一个选中实例。",
        );
    }

    fn migration_confirmation_window(&mut self, ctx: &egui::Context) {
        let Some(target) = self.pending_migration.clone() else {
            return;
        };
        let mut action = None;
        egui::Window::new("确认迁移数据目录")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!("当前:{}", self.store.root.display()));
                ui.label(format!("目标:{}", target.display()));
                ui.label("迁移前会先备份当前配置;源目录不会删除。确认继续?");
                ui.horizontal(|ui| {
                    if ui.button("确认迁移").clicked() {
                        action = Some(true);
                    }
                    if ui.button("取消").clicked() {
                        action = Some(false);
                    }
                });
            });
        if let Some(confirm) = action {
            self.pending_migration = None;
            if confirm {
                self.migrate_data_root(target);
            } else {
                self.setup_notice = Some("已取消数据目录迁移。".into());
            }
        }
    }

    fn home(&mut self, ui: &mut egui::Ui) {
        ui.heading("运行控制");
        if self.setup_required {
            ui.colored_label(
                egui::Color32::YELLOW,
                "首次配置尚未完成：请先进入“首次设置 / 配置修复”完成依赖路径并保存，再运行 dry-run。",
            );
            if ui.button("打开首次设置 / 配置修复").clicked() {
                self.tab = SETUP_TAB;
            }
            ui.separator();
        }
        ui.label("Rust CLI/GUI 迁移第一阶段：默认只进行 dry-run，不执行真实点击。");
        let options = self.instance_options();
        ui.checkbox(
            &mut self.run_all_instances,
            "dry-run 依次运行全部已发现实例",
        );
        if !options.is_empty() {
            egui::ComboBox::from_label("当前实例")
                .selected_text(
                    options
                        .iter()
                        .find(|(index, _)| *index == self.selected_instance_index)
                        .map(|(_, label)| label.as_str())
                        .unwrap_or("未选择"),
                )
                .show_ui(ui, |ui| {
                    for (index, label) in &options {
                        ui.selectable_value(&mut self.selected_instance_index, *index, label);
                    }
                });
        }
        ui.horizontal(|ui| {
            if ui.button("环境诊断").clicked() {
                self.tab = 1;
            }
            if ui.button("运行 dry-run").clicked() {
                if self.setup_required {
                    self.status = "首次配置尚未完成，无法运行；请先保存依赖路径。".into();
                    self.tab = SETUP_TAB;
                } else {
                    self.start_dry_run();
                }
            }
            if ui.button("准备 live 运行").clicked() {
                if self.setup_required {
                    self.status = "首次配置尚未完成，无法准备 live 运行。".into();
                    self.tab = SETUP_TAB;
                } else {
                    self.pending_live_run = true;
                }
            }
            if ui.button("打开配置").clicked() {
                self.tab = 5;
            }
        });
        ui.separator();
        ui.label(format!("配置文件：{}", self.store.path.display()));
        ui.label(format!(
            "APK：{}",
            if self.apk_path.is_empty() {
                "未选择"
            } else {
                &self.apk_path
            }
        ));
        ui.label("正式模式请使用 CLI：zhfd-checkin.exe run --live --confirm");
        if let Some(result) = &self.latest_result {
            ui.separator();
            ui.label(format!(
                "最近结果：{} / {}",
                result.state_history.join(" → "),
                run_result_status(result)
            ));
            ui.label(format!(
                "页面：{}；模式：{}；状态：{}",
                result
                    .page_route
                    .map(|route| route.as_str())
                    .unwrap_or("unknown"),
                result
                    .checkin_mode
                    .map(|mode| mode.as_str())
                    .unwrap_or("unknown"),
                result
                    .checkin_state
                    .map(|state| state.as_str())
                    .unwrap_or("unknown"),
            ));
        }
    }

    fn diagnostics(&mut self, ui: &mut egui::Ui) {
        ui.heading("环境诊断");
        self.render_run_status_card(ui);
        ui.horizontal(|ui| {
            if ui.button("重新检测 LDPlayer / ADB").clicked() {
                let store = self.store.clone();
                let target = self.instance_target();
                self.start_background_job("环境诊断", move || {
                    match diagnose(&store, &target) {
                        Ok(code) => format!("完成，退出码 {:?}", code),
                        Err(e) => format!("失败: {:#}", e),
                    }
                });
            }
            if ui.button("导出诊断报告").clicked() {
                self.export_report();
            }
            if ui.button("导出当前实例 hierarchy").clicked() {
                let store = self.store.clone();
                let target = self.instance_target();
                self.start_background_job("导出 hierarchy", move || {
                    match dump_ui_hierarchy(&store, &target) {
                        Ok(text) => format!("完成，节点 XML 长度 {}", text.len()),
                        Err(error) => format!("失败: {:#}", error),
                    }
                });
            }
            if ui.button("分析截图...").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("图片", &["png", "jpg", "jpeg"])
                    .pick_file()
            {
                match analyze_image_file(&path) {
                    Ok(analysis) => {
                        self.status = format!(
                            "视觉分析：state={:?} center={:?}",
                            analysis.state,
                            analysis.button.map(|button| (button.cx, button.cy))
                        );
                    }
                    Err(error) => self.status = format!("视觉分析失败: {:#}", error),
                }
            }
            if ui.button("刷新最近运行结果").clicked() {
                self.refresh_latest_result();
                self.status = "最近运行结果已刷新".into();
            }
        });
        ui.label("诊断和报告导出不会执行签到点击。报告不包含 Token、Cookie 或定位地址。");
        ui.separator();
        if let Some(path) = &self.report_path {
            ui.label(format!("最近导出的报告：{}", path));
        }
        if let Some(dir) = &self.latest_log_dir {
            ui.label(format!("最近运行日志：{}", dir));
        } else {
            ui.label("尚未找到 result.json");
        }
        if let Some(result) = &self.latest_result {
            ui.horizontal(|ui| {
                ui.label(format!("模式：{}", result.mode));
                ui.label(format!(
                    "Profile：{}",
                    result.profile_id.as_deref().unwrap_or("unknown")
                ));
                ui.label(format!(
                    "页面：{}",
                    result
                        .page_route
                        .map(|route| route.as_str())
                        .unwrap_or("unknown")
                ));
                ui.label(format!(
                    "业务：{}",
                    result
                        .checkin_mode
                        .map(|mode| mode.as_str())
                        .unwrap_or("unknown")
                ));
                ui.label(format!(
                    "签到状态：{}",
                    result
                        .checkin_state
                        .map(|state| state.as_str())
                        .unwrap_or("unknown")
                ));
                ui.label(format!(
                    "点击：{}",
                    if result.clicked { "是" } else { "否" }
                ));
                ui.label(format!("结果：{}", run_result_status(result)));
            });
            if let Some(date) = &result.success_date {
                ui.label(format!("页面日期：{date}"));
            }
            if let Some(error) = &result.error {
                ui.colored_label(
                    egui::Color32::LIGHT_RED,
                    format!("错误：{}（{}）", error, explain_error(error)),
                );
            } else if result.field_work_ready {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "外勤签到状态已识别为 ready；仅 dry-run 观察，未执行点击。",
                );
            } else {
                ui.colored_label(egui::Color32::LIGHT_GREEN, "未记录运行错误");
            }
            if let Some(stage) = &result.error_stage {
                ui.label(format!("失败阶段：{stage}"));
            }
            if let Some(action) = &result.recovery_action {
                ui.label(format!("建议操作：{action}"));
            }
            ui.label(format!("状态历史：{}", result.state_history.join(" → ")));
            ui.label(format!(
                "ADB serial：{}",
                if result.serial.is_empty() {
                    "unknown"
                } else {
                    &result.serial
                }
            ));
        }
        ui.separator();
        ui.label("默认模式为 dry-run。只有普通签到页面可进入 live 点击策略；外勤签到与未知模式保持无点击观察。");
    }

    /// Structured status card for the most recent run.
    ///
    /// Route, page loading, button state, locating state and foreground package
    /// are shown as separate dimensions: "the button is not ready yet" must not
    /// read as "the page is unknown" (requirement FR-016).
    fn render_run_status_card(&self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.label(egui::RichText::new("最近运行状态").strong());
            let Some(result) = &self.latest_result else {
                ui.label("暂无 result.json;先运行一次 dry-run。");
                return;
            };
            let unknown = |value: Option<&str>| value.unwrap_or("unknown").to_string();
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("status={}", unknown(result.status.as_deref())));
                ui.label(format!(
                    "route={}",
                    unknown(
                        result.route.as_deref().or_else(|| {
                            result.page_route.map(crate::domain::PageRoute::as_str)
                        })
                    )
                ));
                ui.label(format!(
                    "mode={}",
                    unknown(result.checkin_mode.map(crate::domain::CheckinMode::as_str))
                ));
                ui.label(format!(
                    "button={}",
                    unknown(result.button_state.as_deref())
                ));
                ui.label(format!(
                    "location={}",
                    unknown(result.location_state.as_deref())
                ));
                ui.label(format!(
                    "page_loading={}",
                    result
                        .page_loading
                        .map(|value| if value { "yes" } else { "no" })
                        .unwrap_or("unknown")
                ));
                ui.label(format!(
                    "hierarchy={}",
                    unknown(result.hierarchy_status.as_deref())
                ));
                if let Some(overlay) = &result.overlay_package {
                    ui.colored_label(
                        egui::Color32::LIGHT_RED,
                        format!("检测到遮挡窗口：{overlay}（弹窗/广告，请手动关闭）"),
                    );
                }
                ui.label(format!(
                    "foreground={}",
                    unknown(result.foreground_package.as_deref())
                ));
                ui.label(format!(
                    "refresh={}/{}",
                    result
                        .page_refresh_attempted
                        .map(|value| if value { "attempted" } else { "not_needed" })
                        .unwrap_or("unknown"),
                    result
                        .page_refresh_confirmed
                        .map(|value| if value { "confirmed" } else { "unconfirmed" })
                        .unwrap_or("unknown")
                ));
            });
            if let Some(code) = &result.error_code {
                ui.colored_label(
                    egui::Color32::LIGHT_RED,
                    format!(
                        "error_code={} ({});stage={}",
                        code,
                        explain_error(code),
                        unknown(result.error_stage.as_deref())
                    ),
                );
            } else {
                ui.colored_label(egui::Color32::LIGHT_GREEN, "未记录机器错误码");
            }
            if let Some(action) = &result.recovery_action {
                ui.label(format!("建议操作:{action}"));
            }
        });
    }

    fn instances_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("模拟器实例");
        if ui.button("刷新实例列表").clicked() {
            match list_instance_summaries(&self.store) {
                Ok(instances) => {
                    self.status = format!("检测到 {} 个实例", instances.len());
                    self.instance_summaries = instances;
                }
                Err(error) => self.status = format!("实例列表失败: {:#}", error),
            }
        }
        if self.instance_summaries.is_empty()
            && let Ok(instances) = list_instance_summaries(&self.store)
        {
            self.instance_summaries = instances;
        }
        egui::Grid::new("instances").striped(true).show(ui, |ui| {
            ui.label("index");
            ui.label("名称");
            ui.label("运行");
            ui.label("TCP ADB serial");
            ui.label("list2 报告端口（参考）");
            ui.label("尺寸/DPI");
            ui.label("Profile");
            ui.label("启用");
            ui.end_row();
            for instance in &self.instance_summaries {
                ui.label(instance.index.to_string());
                ui.label(&instance.name);
                ui.label(if instance.running { "是" } else { "否" });
                ui.label(&instance.serial);
                ui.label(
                    instance
                        .ldplayer_reported_port
                        .map(|port| port.to_string())
                        .unwrap_or_else(|| "未知".into()),
                )
                .on_hover_text(
                    "ldconsole list2 原始报告字段；不是本程序用于 adb connect 的 TCP ADB serial。",
                );
                ui.label(format!(
                    "{}x{}@{}",
                    instance.width.unwrap_or(0),
                    instance.height.unwrap_or(0),
                    instance.dpi.unwrap_or(0)
                ));
                ui.label(instance.profile_id.as_deref().unwrap_or("unknown"));
                ui.label(if instance.enabled { "是" } else { "否" });
                ui.end_row();
            }
        });
    }

    fn profiles(&mut self, ui: &mut egui::Ui) {
        ui.heading("内置 Profile");
        ui.label(format!("当前实例 index：{}", self.selected_instance_index));
        if ui.button("检测当前模拟器 Profile").clicked() {
            let store = self.store.clone();
            let target = self.instance_target();
            self.start_background_job("Profile 检测", move || {
                match runtime_with_target(&store, false, Some(&target)) {
                    Ok((_, device, info)) => {
                        let exact =
                            profile::find_profile(info.width, info.height, info.density_dpi);
                        match exact {
                            Some(p) => format!(
                                "当前 {} {}x{}@{}，匹配 {}",
                                device.serial, info.width, info.height, info.density_dpi, p.id
                            ),
                            None => format!(
                                "当前 {} {}x{}@{}，没有精确匹配 profile",
                                device.serial, info.width, info.height, info.density_dpi
                            ),
                        }
                    }
                    Err(e) => format!("失败: {:#}", e),
                }
            });
        }
        ui.label("已标定 Profile 优先显示；未标定项仍可同步配置，但不允许用于 live 点击。");
        egui::ScrollArea::both()
            .id_salt("profile_table_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(720.0);
                egui::Grid::new("profiles").striped(true).show(ui, |ui| {
                    ui.label("ID");
                    ui.label("尺寸");
                    ui.label("DPI");
                    ui.label("方向");
                    ui.label("运行资格");
                    ui.label("操作");
                    ui.end_row();
                    for p in ordered_profiles() {
                        ui.label(p.id);
                        ui.label(format!("{}x{}", p.width, p.height));
                        ui.label(p.density_dpi.to_string());
                        ui.label(p.orientation.as_str());
                        ui.label(if p.calibrated {
                            "已标定"
                        } else {
                            "仅 dry-run"
                        });
                        if ui.button(format!("准备同步##{}", p.id)).clicked() {
                            self.pending_sync = Some(p.id.to_string());
                        }
                        ui.end_row();
                    }
                });
            });
    }

    fn live_confirmation_window(&mut self, ctx: &egui::Context) {
        if !self.pending_live_run {
            return;
        }
        let mut action = None;
        egui::Window::new("确认正式 live 运行")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                let scope = if self.run_all_instances {
                    "全部已启用实例"
                } else {
                    "当前选择的实例"
                };
                ui.colored_label(egui::Color32::YELLOW, format!("范围：{}", scope));
                ui.label("将执行真实签到点击。程序仍会检查 calibrated Profile、时间窗口、前台包名、连续 ready 和点击前二次确认。");
                ui.label("请确认设备、实例和当前页面均正确；确认后将调用 CLI 同一套 run --live --confirm 业务逻辑。");
                ui.horizontal(|ui| {
                    if ui.button("确认 live").clicked() {
                        action = Some(true);
                    }
                    if ui.button("取消").clicked() {
                        action = Some(false);
                    }
                });
            });
        if let Some(confirm) = action {
            self.pending_live_run = false;
            if confirm {
                self.start_run(true);
            } else {
                self.status = "已取消 live 运行".into();
            }
        }
    }

    fn sync_confirmation_window(&mut self, ctx: &egui::Context) {
        let Some(id) = self.pending_sync.clone() else {
            return;
        };

        let mut action = None;
        egui::Window::new("确认 Profile 同步")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    format!("将修改 LDPlayer 分辨率/DPI 并重启实例：{}", id),
                );
                ui.label("同步会先备份已有配置；执行期间请不要手动操作模拟器。\n确认继续吗？");
                ui.horizontal(|ui| {
                    if ui.button("确认同步").clicked() {
                        action = Some(true);
                    }
                    if ui.button("取消").clicked() {
                        action = Some(false);
                    }
                });
            });

        if let Some(confirm) = action {
            self.pending_sync = None;
            if confirm {
                let store = self.store.clone();
                let sync_id = id.clone();
                let target = self.instance_target();
                self.start_background_job(format!("同步 {}", id), move || {
                    match sync_profile(&store, &sync_id, true, &target) {
                        Ok(code) => format!("完成，退出码 {:?}", code),
                        Err(e) => format!("失败: {:#}", e),
                    }
                });
            } else {
                self.status = "已取消 Profile 同步".into();
            }
        }
    }

    fn tasks_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("计划任务");
        ui.label("计划任务默认创建为 dry-run；正式模式必须显式勾选并确认。\n");
        ui.horizontal(|ui| {
            ui.label("任务名称");
            ui.text_edit_singleline(&mut self.task_name);
        });
        ui.checkbox(&mut self.task_live, "创建 live 任务（需要显式确认）");
        ui.checkbox(&mut self.task_all_instances, "计划任务依次运行全部配置实例");
        ui.horizontal(|ui| {
            if ui.button("查询任务").clicked() {
                let name = self.task_name.clone();
                let config_path = self.store.path.clone();
                self.start_background_job("计划任务查询", move || {
                    match task_command(TaskCommand::Query { name }, Some(&config_path)) {
                        Ok(code) => format!("完成，退出码 {:?}", code),
                        Err(error) => format!("失败: {:#}", error),
                    }
                });
            }
            if ui.button("创建任务").clicked() {
                let name = self.task_name.clone();
                let live = self.task_live;
                let all_instances = self.task_all_instances;
                let config_path = self.store.path.clone();
                let target = self.instance_target();
                if live {
                    self.status = "live 计划任务需要再次确认；请点击下方确认按钮".into();
                } else {
                    self.start_background_job("创建 dry-run 计划任务", move || {
                        match task_command(
                            TaskCommand::Create {
                                name,
                                live: false,
                                confirm: false,
                                all_instances,
                                target,
                            },
                            Some(&config_path),
                        ) {
                            Ok(code) => format!("完成,退出码 {:?}", code),
                            Err(error) => format!("失败: {:#}", error),
                        }
                    });
                }
            }
            if ui.button("删除任务").clicked() {
                let name = self.task_name.clone();
                let config_path = self.store.path.clone();
                self.start_background_job("删除计划任务", move || {
                    match task_command(TaskCommand::Delete { name }, Some(&config_path)) {
                        Ok(code) => format!("完成，退出码 {:?}", code),
                        Err(error) => format!("失败: {:#}", error),
                    }
                });
            }
        });
        if self.task_live {
            ui.separator();
            ui.colored_label(
                egui::Color32::YELLOW,
                "正式计划任务会每天在签到窗口开始时间执行 live；必须再次确认。",
            );
            if ui.button("确认创建 live 计划任务").clicked() {
                let name = self.task_name.clone();
                let all_instances = self.task_all_instances;
                let target = self.instance_target();
                let config_path = self.store.path.clone();
                self.start_background_job("创建 live 计划任务", move || {
                    match task_command(
                        TaskCommand::Create {
                            name,
                            live: true,
                            confirm: true,
                            all_instances,
                            target,
                        },
                        Some(&config_path),
                    ) {
                        Ok(code) => format!("完成，退出码 {:?}", code),
                        Err(error) => format!("失败: {:#}", error),
                    }
                });
                self.task_live = false;
            }
        }
        ui.separator();
        ui.label(format!(
            "配置窗口：{}–{}",
            self.store.config.window.start, self.store.config.window.end
        ));
        ui.label("GUI 不提供直接 live 签到按钮；计划任务 live 仍受 CLI 的全部门禁保护。");
    }

    fn logs_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("运行日志与结果");
        ui.horizontal_wrapped(|ui| {
            if ui.button("刷新列表").clicked() {
                self.refresh_logs();
            }
            if ui.button("打开 logs 目录").clicked() {
                match crate::process::open_path(&self.store.root.join("logs")) {
                    Ok(()) => self.status = "已打开 logs 目录".into(),
                    Err(error) => self.status = format!("打开 logs 目录失败: {error:#}"),
                }
            }
            if ui.button("导出诊断报告").clicked() {
                self.export_report();
            }
            if ui.button("打开最近报告").clicked() {
                match self.report_path.as_ref().map(std::path::PathBuf::from) {
                    Some(path) => match crate::process::open_path(&path) {
                        Ok(()) => self.status = "已打开诊断报告".into(),
                        Err(error) => self.status = format!("打开诊断报告失败: {error:#}"),
                    },
                    None => self.status = "请先导出诊断报告".into(),
                }
            }
            ui.add_space(10.0);
            ui.label("筛选");
            ui.add(
                egui::TextEdit::singleline(&mut self.logs_filter)
                    .hint_text("类型、状态、摘要或路径")
                    .desired_width(210.0),
            );
        });

        // Keep evidence operations outside the scrolling table so they remain
        // reachable while browsing long directories and event summaries.
        ui.horizontal_wrapped(|ui| {
            if let Some(directory) = self.selected_log_directory.clone() {
                ui.add(egui::Label::new(format!("选中：{}", directory.display())).truncate());
                if ui.button("打开目录").clicked() {
                    match crate::process::open_path(&directory) {
                        Ok(()) => self.status = "已打开选中的日志目录".into(),
                        Err(error) => self.status = format!("打开日志目录失败: {error:#}"),
                    }
                }
                if ui.button("复制路径").clicked() {
                    ui.ctx().copy_text(directory.display().to_string());
                    self.status = "日志目录路径已复制".into();
                }
                if ui.button("复制结果/证据").clicked() {
                    if let Some(preview) = &self.selected_log_preview {
                        ui.ctx().copy_text(preview.clone());
                        self.status = "选中的结果/证据文本已复制".into();
                    } else {
                        self.status = "选中目录没有可复制的证据文件".into();
                    }
                }
                if ui.button("导出选中 JSON/证据").clicked() {
                    let file_name = self
                        .selected_log_artifact
                        .as_ref()
                        .and_then(|path| path.file_name())
                        .unwrap_or_else(|| std::ffi::OsStr::new("result.json"))
                        .to_string_lossy()
                        .into_owned();
                    if let Some(destination) =
                        rfd::FileDialog::new().set_file_name(file_name).save_file()
                    {
                        match logger::export_log_artifact(&directory, &destination) {
                            Ok(path) => self.status = format!("日志证据已导出：{}", path.display()),
                            Err(error) => self.status = format!("导出日志证据失败：{error:#}"),
                        }
                    }
                }
                if directory.join("events.jsonl").is_file()
                    && ui.button("导出 events.jsonl").clicked()
                    && let Some(destination) = rfd::FileDialog::new()
                        .set_file_name("events.jsonl")
                        .save_file()
                {
                    match logger::export_log_artifact_named(
                        &directory,
                        "events.jsonl",
                        &destination,
                    ) {
                        Ok(path) => self.status = format!("事件日志已导出：{}", path.display()),
                        Err(error) => self.status = format!("导出事件日志失败：{error:#}"),
                    }
                }
            } else {
                ui.label("选择一条日志以查看和导出结果。");
            }
        });

        let filter = self.logs_filter.trim().to_lowercase();
        let filtered: Vec<logger::LogSummary> = self
            .recent_logs
            .iter()
            .filter(|entry| {
                filter.is_empty()
                    || entry.kind.to_lowercase().contains(&filter)
                    || entry.status.to_lowercase().contains(&filter)
                    || entry.summary.to_lowercase().contains(&filter)
                    || entry
                        .directory
                        .to_string_lossy()
                        .to_lowercase()
                        .contains(filter.as_str())
            })
            .cloned()
            .collect();

        if filtered.is_empty() {
            ui.separator();
            ui.label(if self.recent_logs.is_empty() {
                "尚未找到运行或同步日志。"
            } else {
                "没有匹配当前筛选条件的日志。"
            });
            // The filter only narrows the run list; the structured event
            // timeline belongs to the still-selected run and must stay visible.
            self.render_event_timeline(ui);
            return;
        }

        let mut clicked_directory = None;
        let selected_directory = self.selected_log_directory.clone();
        let table_height = (ui.available_height() * 0.55).clamp(150.0, 360.0);
        egui::ScrollArea::both()
            .id_salt("run_evidence_table_scroll")
            .max_height(table_height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(1080.0);
                egui::Grid::new("recent_logs")
                    .striped(true)
                    .min_col_width(88.0)
                    .show(ui, |ui| {
                        for heading in ["类型", "状态", "摘要", "日志目录"] {
                            ui.label(egui::RichText::new(heading).strong());
                        }
                        ui.end_row();
                        for entry in &filtered {
                            let selected = selected_directory.as_ref() == Some(&entry.directory);
                            let response = ui.selectable_label(selected, &entry.kind);
                            if response.clicked() || response.double_clicked() {
                                clicked_directory = Some(entry.directory.clone());
                            }
                            ui.label(&entry.status);
                            let response = ui.add_sized(
                                [430.0, 24.0],
                                egui::Button::selectable(selected, &entry.summary).truncate(),
                            );
                            if response.clicked() || response.double_clicked() {
                                clicked_directory = Some(entry.directory.clone());
                            }
                            ui.add_sized(
                                [420.0, 24.0],
                                egui::Label::new(entry.directory.display().to_string()).truncate(),
                            );
                            ui.end_row();
                        }
                    });
            });
        if let Some(directory) = clicked_directory {
            self.select_log_directory(directory);
        }

        self.render_event_timeline(ui);

        if let (Some(artifact), Some(preview)) =
            (&self.selected_log_artifact, &self.selected_log_preview)
        {
            ui.horizontal(|ui| {
                ui.label("证据预览");
                ui.add(egui::Label::new(artifact.display().to_string()).truncate());
            });
            egui::ScrollArea::both()
                .id_salt("selected_log_preview")
                .max_height((ui.available_height() - 28.0).max(90.0))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(preview).monospace()).wrap());
                });
        }
    }

    /// Structured event timeline for the selected run.
    ///
    /// Foreground snapshots, hierarchy observations, route decisions and
    /// navigation actions share one ordered view so a route decision can be
    /// read together with the same round's evidence (requirement FR-012).
    fn render_event_timeline(&self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label(egui::RichText::new("事件时间线(前台 / 层级 / 路由 / 导航)").strong());
        if self.recent_events.is_empty() {
            ui.label("选中目录没有可展示的结构化事件(events.jsonl)。");
            return;
        }
        let height = (ui.available_height() * 0.35).clamp(120.0, 300.0);
        egui::ScrollArea::both()
            .id_salt("event_timeline")
            .max_height(height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(760.0);
                egui::Grid::new("event_timeline_grid")
                    .striped(true)
                    .min_col_width(96.0)
                    .show(ui, |ui| {
                        for heading in ["时间", "事件", "摘要"] {
                            ui.label(egui::RichText::new(heading).strong());
                        }
                        ui.end_row();
                        for event in &self.recent_events {
                            ui.label(&event.at);
                            ui.label(&event.kind);
                            ui.add_sized(
                                [520.0, 20.0],
                                egui::Label::new(&event.summary).truncate(),
                            )
                            .on_hover_text(&event.summary);
                            ui.end_row();
                        }
                    });
            });
    }

    fn apk_install_confirmation_window(&mut self, ctx: &egui::Context) {
        let Some(apk) = self.pending_apk_install.clone() else {
            return;
        };
        let mut action = None;
        egui::Window::new("确认安装 APK")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!("APK：{}", apk.display()));
                if let Some(metadata) = &self.pending_apk_metadata {
                    ui.label(format!("APK 包名：{}", metadata.package_name));
                    ui.label(format!(
                        "APK versionName：{}",
                        metadata.version_name.as_deref().unwrap_or("未知")
                    ));
                    ui.label(format!(
                        "APK versionCode：{}",
                        metadata.version_code.as_deref().unwrap_or("未知")
                    ));
                }
                ui.label(format!("目标包名：{}", self.store.config.app.package_name));
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "将连接当前模拟器并使用 adb install -r 安装/更新 APK。",
                );
                ui.horizontal(|ui| {
                    if ui.button("确认安装").clicked() {
                        action = Some(true);
                    }
                    if ui.button("取消").clicked() {
                        action = Some(false);
                    }
                });
            });

        if let Some(confirm) = action {
            self.pending_apk_install = None;
            self.pending_apk_metadata = None;
            if confirm {
                let store = self.store.clone();
                let target = self.instance_target();
                self.start_background_job_with_kind("APK 安装", JobKind::AppInstall, move || {
                    match install_apk(&store, &apk, &target) {
                        Ok(result) => (
                            format!(
                                "完成：{} {}；adb 输出：{}",
                                result.package_name,
                                result.version,
                                result.install_output.trim()
                            ),
                            Some(result.version),
                        ),
                        Err(error) => (format!("失败: {:#}", error), None),
                    }
                });
            } else {
                self.status = "已取消 APK 安装".into();
            }
        }
    }

    fn app_page(&mut self, ui: &mut egui::Ui) {
        if !self.app_version_checked && self.job.is_none() {
            self.start_app_version_query();
        }
        ui.heading("应用管理");
        ui.label(format!("目标包名：{}", self.store.config.app.package_name));
        ui.horizontal(|ui| {
            ui.label("当前已安装版本");
            ui.label(self.installed_version.as_deref().unwrap_or("尚未读取"));
            if ui.button("刷新版本").clicked() {
                self.start_app_version_query();
            }
        });
        ui.horizontal(|ui| {
            ui.label("APK 路径");
            ui.text_edit_singleline(&mut self.apk_path);
            if ui.button("浏览 APK...").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Android APK", &["apk"])
                    .pick_file()
            {
                self.save_apk_path(path);
            }
        });
        if ui.button("保存 APK 路径").clicked() {
            let path = std::path::PathBuf::from(&self.apk_path);
            self.store.config.app.apk_path = config::path_for_config(&self.store.root, &path);
            match self.store.save() {
                Ok(()) => self.status = "APK 路径已保存".into(),
                Err(e) => self.status = format!("保存失败: {:#}", e),
            }
        }
        if ui.button("安装/更新选中的 APK").clicked() {
            match config::resolve_path(&self.store.root, &self.apk_path) {
                Some(apk) if apk.is_file() => match inspect_apk_metadata(&apk) {
                    Ok(metadata) if metadata.package_name == self.store.config.app.package_name => {
                        self.pending_apk_metadata = Some(metadata);
                        self.pending_apk_install = Some(apk);
                    }
                    Ok(metadata) => {
                        self.status = format!(
                            "APK 包名不匹配：实际={}，目标={}",
                            metadata.package_name, self.store.config.app.package_name
                        )
                    }
                    Err(error) => self.status = format!("APK 检查失败: {:#}", error),
                },
                Some(apk) => self.status = format!("APK 不存在：{}", apk.display()),
                None => self.status = "请先选择 APK".into(),
            }
        }
        if let Some(result) = &self.apk_install_result {
            ui.separator();
            ui.label(format!("最近一次安装结果：{}", result));
        }
        ui.label("当前阶段支持标准 .apk；不自动下载、不修改账号或设备身份。安装前必须确认，安装后会校验目标包名和版本。");
    }

    fn config_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("配置");
        ui.label(format!("配置文件：{}", self.store.path.display()));
        ui.horizontal(|ui| {
            if ui.button("扫描 LDPlayer / ADB 依赖").clicked() {
                self.start_dependency_scan();
            }
            if ui.button("保存选中的发现路径").clicked() {
                let Some(report) = self.dependency_report.clone() else {
                    self.status = "请先扫描依赖，再选择并保存候选路径。".into();
                    return;
                };
                let original_ldconsole_path = self.store.config.emulator.ldconsole_path.clone();
                let original_adb_path = self.store.config.emulator.adb_path.clone();
                let mut changed = false;
                if let Some(path) = &self.selected_ldplayer_candidate
                    && report
                        .ldplayer
                        .candidates
                        .iter()
                        .any(|candidate| candidate.exists && &candidate.path == path)
                {
                    self.store.config.emulator.ldconsole_path =
                        config::path_for_config(&self.store.root, path);
                    changed = true;
                }
                if let Some(path) = &self.selected_adb_candidate
                    && report
                        .adb
                        .candidates
                        .iter()
                        .any(|candidate| candidate.exists && &candidate.path == path)
                {
                    self.store.config.emulator.adb_path =
                        config::path_for_config(&self.store.root, path);
                    changed = true;
                }
                self.dependency_report = Some(report.clone());
                if changed {
                    match self.store.save() {
                        Ok(()) => self.status = "已保存选中的 LDPlayer / ADB 路径".into(),
                        Err(error) => {
                            self.store.config.emulator.ldconsole_path = original_ldconsole_path;
                            self.store.config.emulator.adb_path = original_adb_path;
                            self.status = format!("保存发现路径失败：{error:#}");
                        }
                    }
                } else {
                    self.status =
                        format!("{}\n{}", report.ldplayer.summary(), report.adb.summary());
                }
            }
            if ui.button("打开 config.toml").clicked() {
                match crate::config_command(&self.store, ConfigCommand::Open) {
                    Ok(_) => self.status = "已打开 config.toml".into(),
                    Err(error) => self.status = format!("打开配置失败: {error:#}"),
                }
            }
        });
        ui.label(
            "Windows 路径推荐使用 TOML 单引号 literal string，无需双写反斜杠；正斜杠路径也可用。",
        );
        if let Some(report) = &self.dependency_report {
            ui.separator();
            ui.heading("依赖发现结果");
            Self::dependency_candidate_selector(
                ui,
                "LDPlayer",
                &report.ldplayer,
                &mut self.selected_ldplayer_candidate,
            );
            Self::dependency_candidate_selector(
                ui,
                "ADB",
                &report.adb,
                &mut self.selected_adb_candidate,
            );
        }
        ui.separator();
        ui.label("当前配置内容");
        ui.code(toml::to_string_pretty(&self.store.config).unwrap_or_default());
    }

    fn dependency_candidate_selector(
        ui: &mut egui::Ui,
        label: &str,
        report: &crate::discovery::DiscoveryReport,
        selected: &mut Option<std::path::PathBuf>,
    ) {
        let selected_text = report
            .candidates
            .iter()
            .find(|candidate| candidate.exists && Some(&candidate.path) == selected.as_ref())
            .map(|candidate| format!("{} [{}]", candidate.path.display(), candidate.source))
            .unwrap_or_else(|| "没有可选的已安装候选".into());
        egui::ComboBox::from_label(label)
            .selected_text(selected_text)
            .show_ui(ui, |ui| {
                for candidate in report
                    .candidates
                    .iter()
                    .filter(|candidate| candidate.exists)
                {
                    ui.selectable_value(
                        selected,
                        Some(candidate.path.clone()),
                        format!("{} [{}]", candidate.path.display(), candidate.source),
                    );
                }
            });
        ui.collapsing(format!("{label} 候选诊断"), |ui| {
            for candidate in report.candidates.iter().take(20) {
                let marker = if candidate.exists { "✓" } else { "·" };
                ui.label(format!(
                    "{marker} {} [{}]",
                    candidate.path.display(),
                    candidate.source
                ));
            }
            let omitted = report.candidates.len().saturating_sub(20);
            if omitted > 0 {
                ui.label(format!("另有 {omitted} 个候选未展开。"));
            }
        });
    }

    fn about(&mut self, ui: &mut egui::Ui) {
        ui.heading("关于 / About");
        ui.label(format!("版本：{}", env!("CARGO_PKG_VERSION")));
        ui.label(format!("Git commit：{}", crate::GIT_COMMIT));
        ui.label(format!("Build id：{}", crate::build_id()));
        // A dirty build is not the commit it names; surface it here so a bug
        // report cannot be attributed to a clean build of that commit (FR-014).
        match crate::git_dirty() {
            Some(true) => {
                ui.colored_label(
                    egui::Color32::from_rgb(200, 120, 0),
                    "工作区状态：dirty（包含未提交的已跟踪改动）",
                );
            }
            Some(false) => {
                ui.label("工作区状态：clean");
            }
            None => {
                ui.label("工作区状态：unknown（构建时无法运行 git status）");
            }
        }
        ui.hyperlink_to("雷电模拟器官网", "https://www.ldmnq.com/");
        ui.hyperlink_to("智汇福大官网", "https://app.fzu.edu.cn/fd-app/m/index.html");
        ui.separator();
        ui.label("支持的 Profile：");
        for supported in ordered_profiles() {
            ui.label(format!(
                "{} {}x{}@{} {} calibrated={}",
                supported.id,
                supported.width,
                supported.height,
                supported.density_dpi,
                supported.orientation.as_str(),
                supported.calibrated
            ));
        }
        ui.separator();
        ui.label("第三方依赖：");
        ui.label("image：PNG/JPEG 解码和轻量视觉处理");
        ui.label("eframe/egui：原生 GUI");
        ui.label("rfd：APK 文件选择器");
        ui.label("serde / serde_json / toml：配置和结构化日志");
        ui.label("ADB + LDPlayer ldconsole.exe：外部设备控制依赖");
        ui.separator();
        ui.label("使用建议：先执行诊断，再运行 dry-run；正式模式需要显式确认。");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_sort_calibrated_first_with_a_stable_geometry_tiebreaker() {
        let profiles = ordered_profiles();
        assert!(profiles.first().is_some_and(|profile| profile.calibrated));
        assert!(profiles.last().is_some_and(|profile| !profile.calibrated));
        for pair in profiles.windows(2) {
            let left = pair[0];
            let right = pair[1];
            assert!(left.calibrated || !right.calibrated);
            if left.calibrated == right.calibrated {
                let left_key = (
                    left.orientation.as_str(),
                    left.width,
                    left.height,
                    left.density_dpi,
                    left.id,
                );
                let right_key = (
                    right.orientation.as_str(),
                    right.width,
                    right.height,
                    right.density_dpi,
                    right.id,
                );
                assert!(left_key <= right_key);
            }
        }
        let first_order: Vec<_> = profiles.iter().map(|profile| profile.id).collect();
        let second_order: Vec<_> = ordered_profiles()
            .iter()
            .map(|profile| profile.id)
            .collect();
        assert_eq!(first_order, second_order);
    }
    #[test]
    fn all_instances_run_clears_single_instance_target() {
        let target = run_target_for_selection(true, 1);
        assert!(target.instance_index.is_none());
        assert!(target.serial.is_none());
    }

    #[test]
    fn single_instance_run_keeps_selected_index() {
        let target = run_target_for_selection(false, 2);
        assert_eq!(target.instance_index, Some(2));
        assert!(target.serial.is_none());
    }
}
