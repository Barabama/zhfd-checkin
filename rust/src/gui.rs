use crate::commands::{
    ConfigRequest, InstanceTargetArgs, RequestedMode, RunRequest, RunSource, TaskRequest,
};
use crate::config::ConfigStore;
use crate::domain::RunResult;
use crate::ui_text::{self, Key, Lang, t};
use crate::{
    analyze_image_file, dependency_diagnostics, diagnose, dump_ui_hierarchy, inspect_apk_metadata,
    install_apk, installed_app_version, list_instance_summaries, run, runtime_with_target,
    sync_profile, task_command, validate_dependency_selection,
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
    let title = t(Lang::from_config(&store.config.ui.language), Key::AppTitle);
    eframe::run_native(
        title,
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
    pending_live_run: Option<LiveRunIntent>,
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
    /// Reveal the expected account only on explicit request; the default is a
    /// masked field because the value is PII.
    show_auto_login_account: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum JobKind {
    General,
    AppInstall,
    AppVersion,
}

/// What the user reviewed when the live confirmation dialog opened.
///
/// Confirming must run exactly this target. Store/config switches, reset to
/// defaults, or an instance change while the dialog is open would otherwise
/// execute live against something the user never reviewed.
#[derive(Clone, Debug, PartialEq, Eq)]
struct LiveRunIntent {
    config_path: std::path::PathBuf,
    data_root: std::path::PathBuf,
    instance_index: u32,
    all_instances: bool,
}

/// Whether the workspace must stay behind Setup/Repair.
///
/// One predicate for startup, reload, alternate-config selection, and save, so
/// those paths cannot disagree and clear the gate while a dependency path is
/// still empty or the loaded file is a damaged one held in recovery mode.
fn requires_setup(store: &ConfigStore) -> bool {
    store.recovery_error.is_some()
        || store.created_default
        || store.config.emulator.ldconsole_path.trim().is_empty()
        || store.config.emulator.adb_path.trim().is_empty()
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
        let setup_required = startup_error.is_some() || requires_setup(&store);
        let setup_ldconsole = store.config.emulator.ldconsole_path.clone();
        let setup_adb = store.config.emulator.adb_path.clone();
        let setup_error = startup_error.or_else(|| store.recovery_error.clone());
        let migration_target = config::recommended_local_app_data_root()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        let lang = Lang::from_config(&store.config.ui.language);
        let status = if setup_required {
            t(lang, Key::StatusNeedsSetup)
        } else {
            t(lang, Key::StatusReady)
        };
        Self {
            store,
            tab: if setup_required { SETUP_TAB } else { 0 },
            status: status.into(),
            dependency_report: None,
            selected_ldplayer_candidate: None,
            selected_adb_candidate: None,
            dependency_scan: None,
            apk_path,
            job: None,
            pending_sync: None,
            pending_live_run: None,
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
            show_auto_login_account: false,
        }
    }

    /// Current interface language, read from the config every frame so a
    /// switch takes effect immediately.
    fn lang(&self) -> Lang {
        Lang::from_config(&self.store.config.ui.language)
    }

    fn set_language(&mut self, lang: Lang) {
        if self.lang() == lang {
            return;
        }
        self.store.config.ui.language = lang.code().into();
        self.status = match self.store.save() {
            Ok(()) => t(lang, Key::LanguageSaved).into(),
            Err(error) => format!("{}: {error:#}", t(lang, Key::CommonFailed)),
        };
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
            self.status = t(self.lang(), Key::StatusJobBusy).into();
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
        self.status = t(self.lang(), Key::StatusJobStarted).replace("{}", &label);
        self.job = Some(GuiJob {
            label,
            kind,
            result: slot,
        });
        true
    }

    fn start_dependency_scan(&mut self) {
        if self.dependency_scan.is_some() {
            self.status = t(self.lang(), Key::ScanRunning).into();
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
        self.status = t(self.lang(), Key::ScanStarted).into();
    }

    fn instance_target(&self) -> InstanceTargetArgs {
        InstanceTargetArgs {
            instance_index: Some(self.selected_instance_index),
            serial: None,
        }
    }

    /// Snapshot the live target the user is about to review.
    fn live_intent(&self) -> LiveRunIntent {
        LiveRunIntent {
            config_path: self.store.path.clone(),
            data_root: self.store.root.clone(),
            instance_index: self.selected_instance_index,
            all_instances: self.run_all_instances,
        }
    }

    /// Replace the active store after a config switch or data-root migration.
    ///
    /// Everything derived from the previous store is recomputed: the selected
    /// instance, the Setup gate, the setup fields, and the log/result/event
    /// views. Without this the combo could show "未选择" while runs still used
    /// the previous config's instance index, and the status card could keep
    /// rendering results from the old data root.
    fn adopt_store(&mut self, store: ConfigStore) {
        self.setup_ldconsole = store.config.emulator.ldconsole_path.clone();
        self.setup_adb = store.config.emulator.adb_path.clone();
        self.apk_path = store.config.app.apk_path.clone();
        self.selected_instance_index = store
            .config
            .emulator
            .effective_instances()
            .first()
            .map(|instance| instance.instance_index)
            .unwrap_or(store.config.emulator.instance_index);
        self.setup_required = requires_setup(&store);
        self.store = store;
        self.setup_error = None;
        self.latest_result = None;
        self.latest_log_dir = None;
        self.recent_logs.clear();
        self.recent_events.clear();
        self.selected_log_directory = None;
        self.selected_log_artifact = None;
        self.selected_log_preview = None;
        self.refresh_latest_result();
        self.refresh_events();
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
        let lang = self.lang();
        let label = t(
            lang,
            if live {
                Key::ModeLiveShort
            } else {
                Key::ModeDryRunShort
            },
        );
        self.start_background_job(label, move || {
            let request = RunRequest {
                requested_mode: if live {
                    RequestedMode::Live
                } else {
                    RequestedMode::DryRun
                },
                explicit_confirmation: live,
                all_instances,
                json: true,
                target,
            };
            let outcome = if all_instances {
                run(&store, request, RunSource::Gui).map(|code| (code, None))
            } else {
                crate::domain::Mode::from_config(&store.config.runtime.mode).and_then(|mode| {
                    crate::run_single(&store, &request.resolve(mode, RunSource::Gui))
                        .map(|(code, result)| (code, result.error_code))
                })
            };
            match outcome {
                Ok((_, Some(error_code))) if error_code == "route_candidate_unconfirmed" => {
                    t(lang, Key::PageUnconfirmed).into()
                }
                Ok((code, _)) => format!(
                    "{}: {}",
                    t(lang, Key::CommonFinished),
                    ui_text::exit_code_label(lang, code)
                ),
                Err(error) => format!("{}: {error:#}", t(lang, Key::CommonFailed)),
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
            Err(error) => self.status = format!("{}: {error:#}", t(self.lang(), Key::CommonFailed)),
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
            self.status = t(self.lang(), Key::ValidationRunning).into();
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
        self.status = t(self.lang(), Key::ValidationStarted).into();
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
        let lang = self.lang();
        match outcome {
            Ok(summary) => {
                self.setup_error = None;
                self.setup_notice = Some(t(lang, Key::ValidationSummary).replace("{}", &summary));
                self.status = t(lang, Key::ValidationPassed).into();
            }
            Err(error) => {
                self.setup_error = Some(t(lang, Key::ValidationFailed).replace("{}", &error));
                self.setup_notice = None;
                self.status = t(lang, Key::CommonFailed).into();
            }
        }
    }

    fn migrate_data_root(&mut self, destination: std::path::PathBuf) {
        let lang = self.lang();
        match self.store.migrate_to_root(&destination) {
            Ok(store) => {
                // Record only from the GUI entry point: the library migration
                // itself must not touch the real user-level pointer (tests call
                // it). Without the pointer the next start would silently fall
                // back to the source root.
                let recorded = store.record_current_root();
                let root = store.root.clone();
                self.adopt_store(store);
                match recorded {
                    Ok(()) => {
                        self.setup_notice = Some(
                            t(lang, Key::MigrationRecorded)
                                .replace("{}", &root.display().to_string()),
                        );
                        self.status = t(lang, Key::StatusDone).into();
                    }
                    Err(error) => {
                        self.setup_error = Some(
                            t(lang, Key::MigrationRecordFailed)
                                .replacen("{}", &root.display().to_string(), 1)
                                .replacen("{}", &format!("{error:#}"), 1),
                        );
                        self.status = t(lang, Key::MigrationIncomplete).into();
                    }
                }
            }
            Err(error) => {
                // A failed migration must leave the store on the original root.
                self.setup_error = Some(
                    t(lang, Key::MigrationFailed)
                        .replacen("{}", &self.store.root.display().to_string(), 1)
                        .replacen("{}", &format!("{error:#}"), 1),
                );
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
                self.status = format!("{}: {error:#}", t(self.lang(), Key::CommonFailed));
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
            Err(error) => self.status = format!("{}: {error:#}", t(self.lang(), Key::CommonFailed)),
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
                self.status = t(self.lang(), Key::StatusReportExported)
                    .replace("{}", &path.display().to_string());
            }
            Err(error) => self.status = format!("{}: {error:#}", t(self.lang(), Key::CommonFailed)),
        }
    }

    fn save_apk_path(&mut self, path: std::path::PathBuf) {
        self.apk_path = config::path_for_config(&self.store.root, &path);
        self.store.config.app.apk_path = self.apk_path.clone();
        match self.store.save() {
            Ok(()) => {
                self.status = t(self.lang(), Key::StatusApkPathSaved).replace("{}", &self.apk_path)
            }
            Err(e) => self.status = format!("{}: {e:#}", t(self.lang(), Key::CommonFailed)),
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

fn auto_login_status(lang: Lang, result: &RunResult) -> Option<&'static str> {
    if result.auto_login_attempted == Some(true) {
        return Some(match result.auto_login_succeeded {
            Some(true) => t(lang, Key::AutoLoginDone),
            Some(false) => t(lang, Key::AutoLoginUnconfirmed),
            None => t(lang, Key::AutoLoginUnknown),
        });
    }
    if result.auto_login_ready == Some(true) {
        return Some(t(lang, Key::AutoLoginDryReady));
    }
    None
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
        let lang = self.lang();
        egui::SidePanel::left("navigation")
            .resizable(true)
            .default_width(178.0)
            .min_width(142.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.heading(t(lang, Key::AppTitle));
                        ui.label(format!("v{}", env!("CARGO_PKG_VERSION")));
                        ui.label(format!(
                            "{}: {}",
                            t(lang, Key::LanguageLabel),
                            ui_text::language_name(lang)
                        ));
                        ui.separator();
                        ui.label(egui::RichText::new(t(lang, Key::NavWorkspace)).strong());
                        for (index, label) in [
                            (0, Key::NavRun),
                            (1, Key::NavDiagnostics),
                            (2, Key::NavInstances),
                        ] {
                            if ui
                                .selectable_label(self.tab == index, t(lang, label))
                                .clicked()
                            {
                                self.tab = index;
                            }
                        }
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(t(lang, Key::NavBusiness)).strong());
                        for (index, label) in
                            [(3, Key::NavProfiles), (4, Key::NavApp), (5, Key::NavConfig)]
                        {
                            if ui
                                .selectable_label(self.tab == index, t(lang, label))
                                .clicked()
                            {
                                self.tab = index;
                            }
                        }
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(t(lang, Key::NavEvidence)).strong());
                        for (index, label) in [(6, Key::NavTasks), (7, Key::NavLogs)] {
                            if ui
                                .selectable_label(self.tab == index, t(lang, label))
                                .clicked()
                            {
                                self.tab = index;
                                if index == 7 && self.recent_logs.is_empty() {
                                    self.refresh_logs();
                                }
                            }
                        }
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(t(lang, Key::NavHelp)).strong());
                        if ui
                            .selectable_label(self.tab == HELP_TAB, t(lang, Key::NavQuickUse))
                            .clicked()
                        {
                            self.tab = HELP_TAB;
                        }
                        if ui
                            .selectable_label(self.tab == 8, t(lang, Key::NavAbout))
                            .clicked()
                        {
                            self.tab = 8;
                        }
                        // First-run/Repair stays reachable from the normal
                        // workspace after the initial wizard has been left.
                        if ui
                            .selectable_label(self.tab == SETUP_TAB, t(lang, Key::NavSetup))
                            .clicked()
                        {
                            self.tab = SETUP_TAB;
                        }
                    });
            });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.label(&self.status);
            if let Some(job) = &self.job {
                ui.label(t(lang, Key::StatusJobRunning).replace("{}", &job.label));
            }
        });
        egui::CentralPanel::default().show(ctx, |ui| self.page_with_contents(ui));
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

fn page_anchor(ui: &mut egui::Ui, key: Key) {
    if ui.data(|data| data.get_temp::<Key>(egui::Id::new("page_jump"))) == Some(key) {
        let target = egui::Rect::from_min_size(ui.next_widget_position(), egui::Vec2::ZERO);
        ui.scroll_to_rect(target, Some(egui::Align::TOP));
    }
}

fn page_section(ui: &mut egui::Ui, lang: Lang, key: Key) {
    ui.separator();
    page_anchor(ui, key);
    ui.label(egui::RichText::new(t(lang, key)).strong());
}

impl GuiApp {
    fn page_with_contents(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        let tab = self.tab;
        let sections: &[Key] = match tab {
            0 => &[Key::PageRunControls, Key::PageLatestResult],
            1 => &[
                Key::StatusCardTitle,
                Key::PageDiagnosticTools,
                Key::PageLatestResult,
            ],
            2 => &[Key::PageInstanceList],
            3 => &[Key::PagePresetList],
            4 => &[Key::PageInstallPackage],
            5 => &ui_text::CONFIG_HEADINGS,
            6 => &[Key::PageTaskSettings, Key::PageTaskWindow],
            7 => &[Key::PageRunHistory, Key::PageEvents, Key::LogsPreview],
            8 => &[
                Key::AboutLanguage,
                Key::AboutLinks,
                Key::AboutProfiles,
                Key::AboutDeps,
            ],
            HELP_TAB => &[
                Key::QuickPrepareTitle,
                Key::QuickSetupTitle,
                Key::QuickRunTitle,
                Key::QuickIssuesTitle,
                Key::QuickAdvancedTitle,
                Key::SafetyTitle,
            ],
            _ => &[
                Key::EnvTitle,
                Key::SetupDepsHeading,
                Key::SetupRestoreHeading,
                Key::SetupMigrationHeading,
            ],
        };
        let mut jump = None;
        let mut contents = |ui: &mut egui::Ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            for key in std::iter::once(&Key::PageOverview).chain(sections) {
                if *key == Key::LogsPreview && self.selected_log_preview.is_none() {
                    continue;
                }
                if ui.button(t(lang, *key)).clicked() {
                    jump = Some(*key);
                }
            }
        };
        if ui.available_width() >= 880.0 {
            egui::SidePanel::right(egui::Id::new(("page_contents", tab)))
                .exact_width(170.0)
                .resizable(false)
                .show_inside(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.strong(t(lang, Key::PageContents));
                        contents(ui);
                    });
                });
        } else {
            egui::CollapsingHeader::new(t(lang, Key::PageContents))
                .id_salt(("page_contents", tab))
                .show(ui, |ui| ui.horizontal_wrapped(contents));
            ui.separator();
        }

        let jump_id = egui::Id::new("page_jump");
        if let Some(key) = jump {
            ui.data_mut(|data| data.insert_temp(jump_id, key));
        }
        egui::ScrollArea::vertical()
            .id_salt(("page_body", tab))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                page_anchor(ui, Key::PageOverview);
                match tab {
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
                }
            });
        ui.data_mut(|data| data.remove::<Key>(jump_id));
    }

    fn quick_use(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        ui.heading(t(lang, Key::QuickHeading));
        ui.label(t(lang, Key::QuickIntro));
        ui.separator();
        page_anchor(ui, Key::QuickPrepareTitle);
        egui::CollapsingHeader::new(t(lang, ui_text::QUICK_USE_HEADINGS[0]))
            .default_open(true)
            .show(ui, |ui| {
                ui.label(egui::RichText::new(t(lang, Key::EnvTitle)).strong());
                ui.label(t(lang, Key::EnvIntro));
                render_environment_checklist(ui, lang);
                ui.separator();
                ui.label(egui::RichText::new(t(lang, Key::EnvStepsTitle)).strong());
                ui.label(t(lang, Key::EnvStep1));
                ui.label(t(lang, Key::EnvStep2));
                ui.label(t(lang, Key::EnvStep3));
                ui.label(t(lang, Key::EnvStep4));
                ui.label(t(lang, Key::EnvNote));
                ui.separator();
                ui.label(egui::RichText::new(t(lang, Key::TermTitle)).strong());
                egui::Grid::new("term_hints")
                    .striped(true)
                    .min_col_width(150.0)
                    .show(ui, |ui| {
                        for term in ui_text::TERM_HINTS {
                            ui.label(egui::RichText::new(t(lang, term.term)).strong());
                            ui.label(t(lang, term.meaning));
                            ui.end_row();
                        }
                    });
                ui.add_space(6.0);
                ui.label(egui::RichText::new(t(lang, Key::DiagramTitle)).strong());
                ui.code(t(lang, Key::Diagram));
                ui.label(t(lang, Key::TermPortNote));
            });
        page_anchor(ui, Key::QuickSetupTitle);
        egui::CollapsingHeader::new(t(lang, ui_text::QUICK_USE_HEADINGS[1]))
            .default_open(true)
            .show(ui, |ui| {
                ui.label(t(lang, Key::QuickSetupStep1));
                ui.label(t(lang, Key::QuickSetupStep2));
                ui.label(t(lang, Key::QuickSetupStep3));
                ui.label(t(lang, Key::QuickSetupStep4));
                ui.label(t(lang, Key::QuickSetupNoneNeeded));
                ui.horizontal_wrapped(|ui| {
                    if ui.button(t(lang, Key::QuickOpenSetup)).clicked() {
                        self.tab = SETUP_TAB;
                    }
                    if ui.button(t(lang, Key::QuickOpenDiagnostics)).clicked() {
                        self.tab = 1;
                    }
                    if ui.button(t(lang, Key::QuickOpenRun)).clicked() {
                        self.tab = 0;
                    }
                });
            });
        page_anchor(ui, Key::QuickRunTitle);
        egui::CollapsingHeader::new(t(lang, ui_text::QUICK_USE_HEADINGS[2]))
            .default_open(true)
            .show(ui, |ui| {
                ui.label(t(lang, Key::QuickRunDrySuccess));
                ui.label(t(lang, Key::QuickRunLive));
                ui.label(t(lang, Key::QuickRunFailure));
                ui.horizontal_wrapped(|ui| {
                    if ui.button(t(lang, Key::QuickOpenLogs)).clicked() {
                        self.tab = 7;
                        self.refresh_logs();
                    }
                    if ui.button(t(lang, Key::QuickOpenRun)).clicked() {
                        self.tab = 0;
                    }
                });
            });
        page_anchor(ui, Key::QuickIssuesTitle);
        ui.collapsing(t(lang, ui_text::QUICK_USE_HEADINGS[3]), |ui| {
            ui.label(t(lang, Key::QuickIssueDevice));
            ui.label(t(lang, Key::QuickIssueLogin));
            ui.label(t(lang, Key::QuickIssueDialog));
            ui.label(t(lang, Key::QuickIssueMore));
            ui.horizontal_wrapped(|ui| {
                if ui.button(t(lang, Key::QuickOpenLogs)).clicked() {
                    self.tab = 7;
                    self.refresh_logs();
                }
                if ui.button(t(lang, Key::QuickOpenDiagnostics)).clicked() {
                    self.tab = 1;
                }
                if ui.button(t(lang, Key::QuickOpenSetup)).clicked() {
                    self.tab = SETUP_TAB;
                }
            });
        });
        page_anchor(ui, Key::QuickAdvancedTitle);
        ui.collapsing(t(lang, ui_text::QUICK_USE_HEADINGS[4]), |ui| {
            ui.label(t(lang, Key::QuickAdvancedInstances));
            ui.label(t(lang, Key::QuickAdvancedTasks));
            ui.label(t(lang, Key::QuickAdvancedData));
            ui.label(t(lang, Key::QuickAdvancedShare));
            ui.label(t(lang, Key::QuickAdvancedBroken));
            ui.horizontal_wrapped(|ui| {
                if ui.button(t(lang, Key::QuickOpenTasks)).clicked() {
                    self.tab = 6;
                }
                if ui.button(t(lang, Key::QuickOpenConfig)).clicked() {
                    self.tab = 5;
                }
                if ui.button(t(lang, Key::QuickOpenLogs)).clicked() {
                    self.tab = 7;
                    self.refresh_logs();
                }
            });
        });
        ui.separator();
        page_anchor(ui, Key::SafetyTitle);
        render_safety_table(ui, lang);
    }

    /// Setup (first run) and Repair (damaged config) share one page: the same
    /// actions are needed in both cases, and the frozen decisions require a
    /// reachable recovery entry rather than a startup-failure dialog.
    fn setup_page(&mut self, ui: &mut egui::Ui) {
        // Only a genuine load/validation failure makes this a Repair page;
        // a successful reload or tool validation is ordinary feedback.
        let lang = self.lang();
        let repairing = self.setup_error.is_some() || self.store.recovery_error.is_some();
        ui.heading(t(
            lang,
            if repairing {
                Key::SetupRepairHeading
            } else {
                Key::SetupFirstHeading
            },
        ));
        ui.label(
            t(lang, Key::SetupConfigPath).replace("{}", &self.store.path.display().to_string()),
        );
        ui.label(t(lang, Key::SetupDataRoot).replace("{}", &self.store.root.display().to_string()));
        let origin = t(
            lang,
            match self.store.origin {
                config::ConfigOrigin::Explicit => Key::SetupOriginExplicit,
                config::ConfigOrigin::Portable => Key::SetupOriginPortable,
                config::ConfigOrigin::LocalAppData => Key::SetupOriginLocalAppData,
                config::ConfigOrigin::Recorded => Key::SetupOriginRecorded,
            },
        );
        ui.label(t(lang, Key::SetupOrigin).replace("{}", origin));
        ui.separator();
        page_section(ui, lang, Key::EnvTitle);
        render_environment_checklist(ui, lang);
        if let Some(error) = self.setup_error.clone() {
            ui.separator();
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                t(lang, Key::SetupConfigProblem).replace("{}", &error),
            );
            ui.label(t(lang, Key::SetupConfigProblemNote));
            if ui.button(t(lang, Key::SetupCopyError)).clicked() {
                ui.ctx().copy_text(error);
                self.status = t(lang, Key::SetupErrorCopied).into();
            }
        }
        if let Some(notice) = self.setup_notice.clone() {
            ui.separator();
            ui.colored_label(
                egui::Color32::LIGHT_GREEN,
                t(lang, Key::SetupLastAction).replace("{}", &notice),
            );
        }

        ui.separator();
        page_section(ui, lang, Key::SetupDepsHeading);
        ui.horizontal(|ui| {
            ui.label("ldconsole.exe");
            ui.add(
                egui::TextEdit::singleline(&mut self.setup_ldconsole)
                    .desired_width(430.0)
                    .hint_text(t(lang, Key::SetupHintLdconsole)),
            );
            if ui.button(t(lang, Key::SetupBrowse)).clicked()
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
                    self.setup_notice = Some(t(lang, Key::SetupAdbSuggested).into());
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("adb.exe");
            ui.add(
                egui::TextEdit::singleline(&mut self.setup_adb)
                    .desired_width(430.0)
                    .hint_text(t(lang, Key::SetupHintAdb)),
            );
            if ui.button(t(lang, Key::SetupBrowse)).clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("adb.exe", &["exe"])
                    .pick_file()
            {
                self.setup_adb = path.display().to_string();
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button(t(lang, Key::SetupValidate)).clicked() {
                self.start_setup_validation();
            }
            if self.setup_validation.is_some() {
                ui.spinner();
                ui.label(t(lang, Key::SetupValidating));
            }
            if ui.button(t(lang, Key::SetupSave)).clicked() {
                // Empty dependency paths are not a usable setup: refuse the
                // write and keep the gate closed instead of publishing a config
                // that can only fail at run time.
                if self.setup_ldconsole.trim().is_empty() || self.setup_adb.trim().is_empty() {
                    self.setup_error = None;
                    self.setup_notice = Some(t(lang, Key::SetupEmptyPaths).into());
                    self.status = t(lang, Key::SetupPathsIncomplete).into();
                } else {
                    // Persist a candidate and only publish it after the atomic
                    // write succeeded, so a failure keeps the last good config.
                    let mut candidate = self.store.config.clone();
                    candidate.emulator.ldconsole_path = self.setup_ldconsole.clone();
                    candidate.emulator.adb_path = self.setup_adb.clone();
                    match self.store.save_candidate(candidate) {
                        Ok(()) => {
                            // Recompute from the same predicate used at startup;
                            // a successful write is not by itself proof that the
                            // dependencies are usable.
                            self.setup_required = requires_setup(&self.store);
                            self.setup_error = None;
                            self.setup_notice = None;
                            self.status = t(lang, Key::SetupSaved).into();
                            if !self.setup_required {
                                self.tab = 0;
                            }
                        }
                        Err(error) => {
                            // The write is atomic, so both the on-disk config and the
                            // in-memory config are still the previous ones.
                            self.setup_error = Some(
                                t(lang, Key::SetupSaveFailed).replace("{}", &format!("{error:#}")),
                            );
                            self.setup_notice = None;
                        }
                    }
                }
            }
            if ui.button(t(lang, Key::SetupOpenDir)).clicked()
                && let Some(parent) = self.store.path.parent()
                && let Err(error) = crate::process::open_path(parent)
            {
                self.status = t(lang, Key::SetupOpenDirFailed).replace("{}", &format!("{error:#}"));
            }
        });

        ui.separator();
        page_section(ui, lang, Key::SetupRestoreHeading);
        ui.horizontal_wrapped(|ui| {
            if ui.button(t(lang, Key::SetupReload)).clicked() {
                match self.store.reload() {
                    Ok(()) => {
                        self.setup_ldconsole = self.store.config.emulator.ldconsole_path.clone();
                        self.setup_adb = self.store.config.emulator.adb_path.clone();
                        // Reload validates the file, not the dependencies: an
                        // empty tool path must keep Setup open.
                        self.setup_required = requires_setup(&self.store);
                        self.setup_error = None;
                        self.setup_notice = Some(
                            t(
                                lang,
                                if self.setup_required {
                                    Key::SetupReloadedEmpty
                                } else {
                                    Key::SetupReloaded
                                },
                            )
                            .into(),
                        );
                    }
                    Err(error) => {
                        // Reload failure must keep the last valid in-memory config.
                        self.setup_error = Some(
                            t(lang, Key::SetupReloadFailed).replace("{}", &format!("{error:#}")),
                        );
                        self.setup_notice = None;
                    }
                }
            }
            if ui.button(t(lang, Key::SetupChooseOther)).clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("TOML", &["toml"])
                    .pick_file()
            {
                match ConfigStore::load_alternate(&path) {
                    Ok(store) => {
                        self.adopt_store(store);
                        self.setup_notice = Some(t(lang, Key::SetupSwitched).into());
                    }
                    Err(error) => {
                        self.setup_error = Some(
                            t(lang, Key::SetupAltUnavailable).replace("{}", &format!("{error:#}")),
                        );
                        self.setup_notice = None;
                    }
                }
            }
            if ui
                .button(if self.confirm_reset_defaults {
                    t(lang, Key::SetupResetConfirm)
                } else {
                    t(lang, Key::SetupResetDefaults)
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
                            self.setup_notice = Some(t(lang, Key::SetupResetDone).into());
                        }
                        Err(error) => {
                            self.setup_error = Some(
                                t(lang, Key::SetupResetFailed).replace("{}", &format!("{error:#}")),
                            );
                            self.setup_notice = None;
                        }
                    }
                    self.confirm_reset_defaults = false;
                } else {
                    self.confirm_reset_defaults = true;
                    self.status = t(lang, Key::SetupResetNeedsConfirm).into();
                }
            }
        });

        ui.separator();
        page_section(ui, lang, Key::SetupMigrationHeading);
        ui.label(t(lang, Key::SetupMigrationNote));
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.migration_target)
                    .desired_width(430.0)
                    .hint_text(t(lang, Key::SetupMigrationTargetHint)),
            );
            if ui.button(t(lang, Key::SetupChooseDir)).clicked()
                && let Some(path) = rfd::FileDialog::new().pick_folder()
            {
                self.migration_target = path.display().to_string();
            }
            if ui.button(t(lang, Key::SetupMigrate)).clicked() {
                if self.migration_target.trim().is_empty() {
                    self.setup_notice = Some(t(lang, Key::SetupChooseTarget).into());
                } else {
                    self.pending_migration =
                        Some(std::path::PathBuf::from(self.migration_target.trim()));
                }
            }
        });

        ui.separator();
        ui.label(t(lang, Key::SetupLoginNote));
    }

    fn migration_confirmation_window(&mut self, ctx: &egui::Context) {
        let Some(target) = self.pending_migration.clone() else {
            return;
        };
        let lang = self.lang();
        let mut action = None;
        egui::Window::new(t(lang, Key::MigrationTitle))
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(
                    t(lang, Key::MigrationCurrent)
                        .replace("{}", &self.store.root.display().to_string()),
                );
                ui.label(
                    t(lang, Key::MigrationTarget).replace("{}", &target.display().to_string()),
                );
                ui.label(t(lang, Key::MigrationWarning));
                ui.horizontal(|ui| {
                    if ui.button(t(lang, Key::MigrationConfirm)).clicked() {
                        action = Some(true);
                    }
                    if ui.button(t(lang, Key::CommonCancel)).clicked() {
                        action = Some(false);
                    }
                });
            });
        if let Some(confirm) = action {
            self.pending_migration = None;
            if confirm {
                self.migrate_data_root(target);
            } else {
                self.setup_notice = Some(t(lang, Key::MigrationCancelled).into());
            }
        }
    }

    fn home(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        ui.heading(t(lang, Key::HomeHeading));
        if self.setup_required {
            ui.colored_label(egui::Color32::YELLOW, t(lang, Key::HomeSetupBanner));
            if ui.button(t(lang, Key::HomeOpenSetup)).clicked() {
                self.tab = SETUP_TAB;
            }
            ui.separator();
        }
        ui.group(|ui| {
            ui.label(egui::RichText::new(t(lang, Key::HomeAboutTitle)).strong());
            ui.label(t(lang, Key::HomeAboutWhat));
            ui.label(t(lang, Key::HomeAboutDry));
            ui.label(t(lang, Key::HomeAboutLive));
            ui.label(t(lang, Key::HomeAboutUsable).replace(
                "{}",
                t(
                    lang,
                    if self.setup_required {
                        Key::HomeUsableNo
                    } else {
                        Key::HomeUsableYes
                    },
                ),
            ));
        });
        if !self.setup_required && self.latest_result.is_none() {
            ui.colored_label(egui::Color32::LIGHT_BLUE, t(lang, Key::HomeFirstRunTitle));
            ui.label(t(lang, Key::HomeFirstRunSteps));
        }
        page_section(ui, lang, Key::PageRunControls);
        let options = self.instance_options();
        ui.checkbox(&mut self.run_all_instances, t(lang, Key::HomeAllInstances));
        if !options.is_empty() {
            egui::ComboBox::from_label(t(lang, Key::HomeCurrentInstance))
                .selected_text(
                    options
                        .iter()
                        .find(|(index, _)| *index == self.selected_instance_index)
                        .map(|(_, label)| label.as_str())
                        .unwrap_or_else(|| t(lang, Key::HomeNotSelected)),
                )
                .show_ui(ui, |ui| {
                    for (index, label) in &options {
                        ui.selectable_value(&mut self.selected_instance_index, *index, label);
                    }
                });
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button(t(lang, Key::HomeButtonDiagnostics)).clicked() {
                self.tab = 1;
            }
            if ui.button(t(lang, Key::HomeButtonDryRun)).clicked() {
                if self.setup_required {
                    self.status = t(lang, Key::HomeSetupIncomplete).into();
                    self.tab = SETUP_TAB;
                } else {
                    self.start_dry_run();
                }
            }
            if ui.button(t(lang, Key::HomeButtonPrepareLive)).clicked() {
                if self.setup_required {
                    self.status = t(lang, Key::HomeSetupIncompleteLive).into();
                    self.tab = SETUP_TAB;
                } else {
                    self.pending_live_run = Some(self.live_intent());
                }
            }
            if ui.button(t(lang, Key::HomeButtonOpenConfig)).clicked() {
                self.tab = 5;
            }
        });
        ui.separator();
        ui.label(
            t(lang, Key::HomeConfigPath).replace("{}", &self.store.path.display().to_string()),
        );
        let apk = if self.apk_path.is_empty() {
            t(lang, Key::HomeApkNotSelected).to_string()
        } else {
            self.apk_path.clone()
        };
        ui.label(t(lang, Key::HomeApkPath).replace("{}", &apk));
        ui.label(t(lang, Key::HomeAdvancedCli));
        page_section(ui, lang, Key::PageLatestResult);
        if let Some(result) = &self.latest_result {
            ui.label(
                t(lang, Key::HomeLatestResult)
                    .replace("{}", ui_text::run_result_label(lang, Some(result))),
            );
            ui.label(
                t(lang, Key::HomeLatestPage)
                    .replacen("{}", ui_text::page_route_label(lang, result.page_route), 1)
                    .replacen(
                        "{}",
                        ui_text::checkin_mode_label(lang, result.checkin_mode),
                        1,
                    )
                    .replacen(
                        "{}",
                        ui_text::checkin_state_label(lang, result.checkin_state),
                        1,
                    ),
            );
            if result.error_code.as_deref() == Some("route_candidate_unconfirmed") {
                ui.label(ui_text::explain_error(lang, "route_candidate_unconfirmed"));
            }
        } else {
            ui.label(t(lang, Key::DiagNoRun));
        }
    }

    fn diagnostics(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        ui.heading(t(lang, Key::DiagHeading));
        ui.label(t(lang, Key::DiagIntro));
        page_anchor(ui, Key::StatusCardTitle);
        self.render_run_status_card(ui);
        page_section(ui, lang, Key::PageDiagnosticTools);
        ui.horizontal_wrapped(|ui| {
            if ui.button(t(lang, Key::DiagRedetect)).clicked() {
                let store = self.store.clone();
                let target = self.instance_target();
                self.start_background_job(t(lang, Key::DiagHeading), move || {
                    match diagnose(&store, &target) {
                        Ok(code) => format!(
                            "{}: {}",
                            t(lang, Key::CommonFinished),
                            ui_text::exit_code_label(lang, code)
                        ),
                        Err(e) => format!("{}: {e:#}", t(lang, Key::CommonFailed)),
                    }
                });
            }
            if ui.button(t(lang, Key::DiagExportReport)).clicked() {
                self.export_report();
            }
            if ui.button(t(lang, Key::DiagDumpHierarchy)).clicked() {
                let store = self.store.clone();
                let target = self.instance_target();
                self.start_background_job(t(lang, Key::DiagDumpHierarchy), move || {
                    match dump_ui_hierarchy(&store, &target) {
                        Ok(text) => {
                            t(lang, Key::DiagDumpDone).replace("{}", &text.len().to_string())
                        }
                        Err(error) => format!("{}: {error:#}", t(lang, Key::CommonFailed)),
                    }
                });
            }
            if ui.button(t(lang, Key::DiagAnalyzeImage)).clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("图片", &["png", "jpg", "jpeg"])
                    .pick_file()
            {
                match analyze_image_file(&path) {
                    Ok(analysis) => {
                        let state = match analysis.state {
                            crate::vision::ButtonState::Gray => t(lang, Key::ButtonGray),
                            crate::vision::ButtonState::Locating => t(lang, Key::ButtonLocating),
                            crate::vision::ButtonState::Ready => t(lang, Key::ButtonReady),
                            crate::vision::ButtonState::Success => t(lang, Key::ButtonSuccess),
                            crate::vision::ButtonState::Unknown => t(lang, Key::ButtonUnknown),
                        };
                        let mut message = t(lang, Key::DiagImageState).replace("{}", state);
                        if let Some(button) = analysis.button {
                            message.push_str("; ");
                            message.push_str(
                                &t(lang, Key::DiagImageCenter)
                                    .replacen("{}", &button.cx.to_string(), 1)
                                    .replacen("{}", &button.cy.to_string(), 1),
                            );
                        }
                        self.status = message;
                    }
                    Err(error) => {
                        self.status = format!("{}: {error:#}", t(lang, Key::CommonFailed))
                    }
                }
            }
            if ui.button(t(lang, Key::DiagRefreshResult)).clicked() {
                self.refresh_latest_result();
                self.status = t(lang, Key::DiagResultRefreshed).into();
            }
        });
        ui.label(t(lang, Key::DiagNote));
        page_section(ui, lang, Key::PageLatestResult);
        if let Some(path) = &self.report_path {
            ui.label(t(lang, Key::DiagReportPath).replace("{}", path));
        }
        if let Some(dir) = &self.latest_log_dir {
            ui.label(t(lang, Key::DiagLatestLog).replace("{}", dir));
        } else {
            ui.label(t(lang, Key::DiagNoRun));
        }
        if let Some(result) = &self.latest_result {
            let mode = t(
                lang,
                if result.mode == "live" {
                    Key::ModeLiveShort
                } else {
                    Key::ModeDryRunShort
                },
            );
            let profile = result
                .profile_id
                .clone()
                .unwrap_or_else(|| t(lang, Key::CommonNotRead).to_string());
            let serial = if result.serial.is_empty() {
                t(lang, Key::CommonNotRead)
            } else {
                result.serial.as_str()
            };
            ui.horizontal_wrapped(|ui| {
                ui.label(t(lang, Key::DiagMode).replace("{}", mode));
                ui.label(t(lang, Key::DiagProfile).replace("{}", &profile));
                ui.label(
                    t(lang, Key::DiagPage)
                        .replace("{}", ui_text::page_route_label(lang, result.page_route)),
                );
                ui.label(
                    t(lang, Key::DiagCheckinType)
                        .replace("{}", ui_text::checkin_mode_label(lang, result.checkin_mode)),
                );
                ui.label(t(lang, Key::DiagCheckinState).replace(
                    "{}",
                    ui_text::checkin_state_label(lang, result.checkin_state),
                ));
                ui.label(t(lang, Key::DiagClicked).replace(
                    "{}",
                    t(
                        lang,
                        if result.clicked {
                            Key::CommonYes
                        } else {
                            Key::CommonNo
                        },
                    ),
                ));
                ui.label(format!(
                    "{}: {}",
                    t(lang, Key::StatusLabelStatus),
                    ui_text::run_result_label(lang, Some(result))
                ));
                if let Some(auto_login) = auto_login_status(lang, result) {
                    ui.label(t(lang, Key::DiagAutoLogin).replace("{}", auto_login));
                }
            });
            if let Some(date) = &result.success_date {
                ui.label(t(lang, Key::DiagPageDate).replace("{}", date));
            }
            if let Some(error) = &result.error {
                // `result.error` is the human-readable detail for a propagated
                // failure, so the hint must be looked up from the *machine code*
                // first; otherwise every classified wait failure falls through
                // to the generic hint.
                let hint = result
                    .error_code
                    .as_deref()
                    .map(|code| ui_text::explain_error(lang, code))
                    .unwrap_or_else(|| ui_text::explain_error(lang, error));
                ui.colored_label(
                    egui::Color32::LIGHT_RED,
                    t(lang, Key::DiagError)
                        .replacen("{}", error, 1)
                        .replacen("{}", hint, 1),
                );
            } else if result.field_work_ready {
                ui.colored_label(egui::Color32::YELLOW, t(lang, Key::DiagFieldWorkReady));
            } else {
                ui.colored_label(egui::Color32::LIGHT_GREEN, t(lang, Key::DiagNoError));
            }
            if let Some(action) = &result.recovery_action {
                ui.label(t(lang, Key::DiagAction).replace("{}", action));
            }
            ui.collapsing(t(lang, Key::StatusTechnical), |ui| {
                if let Some(code) = &result.error_code {
                    ui.label(t(lang, Key::StatusErrorCode).replace("{}", code));
                }
                if let Some(stage) = &result.error_stage {
                    ui.label(t(lang, Key::DiagStage).replace("{}", stage));
                }
                ui.label(
                    t(lang, Key::DiagHistory).replace("{}", &result.state_history.join(" -> ")),
                );
                ui.label(t(lang, Key::DiagSerial).replace("{}", serial));
            });
        }
        ui.separator();
        ui.label(t(lang, Key::DiagDefaultNote));
    }

    /// Structured status card for the most recent run.
    ///
    /// Route, page loading, button state, locating state and foreground package
    /// are shown as separate dimensions: "the button is not ready yet" must not
    /// read as "the page is unknown" (requirement FR-016). Raw machine values
    /// live only inside the collapsed technical-details section.
    fn render_run_status_card(&self, ui: &mut egui::Ui) {
        let lang = self.lang();
        ui.group(|ui| {
            ui.label(egui::RichText::new(t(lang, Key::StatusCardTitle)).strong());
            let Some(result) = &self.latest_result else {
                ui.label(t(lang, Key::StatusCardEmpty));
                return;
            };
            egui::Grid::new("run_status_card")
                .striped(true)
                .min_col_width(110.0)
                .show(ui, |ui| {
                    for (label, value) in [
                        (
                            Key::StatusLabelStatus,
                            ui_text::run_result_label(lang, Some(result)).to_string(),
                        ),
                        (
                            Key::StatusLabelRoute,
                            ui_text::page_route_label(lang, result.page_route).to_string(),
                        ),
                        (
                            Key::StatusLabelMode,
                            ui_text::checkin_mode_label(lang, result.checkin_mode).to_string(),
                        ),
                        (
                            Key::StatusLabelButton,
                            ui_text::button_state_label(lang, result.button_state.as_deref())
                                .to_string(),
                        ),
                        (
                            Key::StatusLabelLocation,
                            ui_text::location_state_label(lang, result.location_state.as_deref())
                                .to_string(),
                        ),
                        (
                            Key::StatusLabelLoading,
                            ui_text::page_loading_label(lang, result.page_loading).to_string(),
                        ),
                        (
                            Key::StatusLabelHierarchy,
                            ui_text::hierarchy_status_label(
                                lang,
                                result.hierarchy_status.as_deref(),
                            )
                            .to_string(),
                        ),
                        (
                            Key::StatusLabelForeground,
                            result
                                .foreground_package
                                .clone()
                                .unwrap_or_else(|| t(lang, Key::CommonNotRead).to_string()),
                        ),
                        (
                            Key::StatusLabelRefresh,
                            ui_text::refresh_label(
                                lang,
                                result.page_refresh_attempted,
                                result.page_refresh_confirmed,
                            ),
                        ),
                        (
                            Key::StatusLabelClicked,
                            t(
                                lang,
                                if result.clicked {
                                    Key::StatusClicked
                                } else {
                                    Key::StatusNotClicked
                                },
                            )
                            .to_string(),
                        ),
                    ] {
                        ui.label(t(lang, label));
                        ui.label(value);
                        ui.end_row();
                    }
                });
            if let Some(overlay) = &result.overlay_package {
                ui.colored_label(
                    egui::Color32::LIGHT_RED,
                    t(lang, Key::StatusOverlay).replace("{}", overlay),
                );
            }
            if let Some(code) = &result.error_code {
                ui.colored_label(egui::Color32::LIGHT_RED, ui_text::explain_error(lang, code));
            } else {
                ui.colored_label(egui::Color32::LIGHT_GREEN, t(lang, Key::StatusNoErrorCode));
            }
            if let Some(action) = &result.recovery_action {
                ui.label(t(lang, Key::StatusRecovery).replace("{}", action));
            }
            ui.collapsing(t(lang, Key::StatusTechnical), |ui| {
                if let Some(code) = &result.error_code {
                    ui.label(t(lang, Key::StatusErrorCode).replace("{}", code));
                }
                if let Some(stage) = &result.error_stage {
                    ui.label(t(lang, Key::StatusStage).replace("{}", stage));
                }
                ui.label(
                    t(lang, Key::DiagHistory).replace("{}", &result.state_history.join(" -> ")),
                );
                let serial = if result.serial.is_empty() {
                    t(lang, Key::CommonNotRead)
                } else {
                    result.serial.as_str()
                };
                ui.label(t(lang, Key::DiagSerial).replace("{}", serial));
            });
        });
    }

    fn instances_page(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        ui.heading(t(lang, Key::InstHeading));
        ui.label(t(lang, Key::InstIntro));
        if ui.button(t(lang, Key::InstRefresh)).clicked() {
            match list_instance_summaries(&self.store) {
                Ok(instances) => {
                    self.status = t(lang, Key::StatusInstancesFound)
                        .replace("{}", &instances.len().to_string());
                    self.instance_summaries = instances;
                }
                Err(error) => self.status = format!("{}: {error:#}", t(lang, Key::CommonFailed)),
            }
        }
        if self.instance_summaries.is_empty()
            && let Ok(instances) = list_instance_summaries(&self.store)
        {
            self.instance_summaries = instances;
        }
        page_section(ui, lang, Key::PageInstanceList);
        egui::ScrollArea::horizontal().show(ui, |ui| {
            egui::Grid::new("instances").striped(true).show(ui, |ui| {
                ui.label(t(lang, Key::InstColIndex));
                ui.label(t(lang, Key::InstColName));
                ui.label(t(lang, Key::InstColRunning));
                ui.label(t(lang, Key::InstColSerial));
                ui.label(t(lang, Key::InstColPort));
                ui.label(t(lang, Key::InstColGeometry));
                ui.label(t(lang, Key::InstColProfile));
                ui.label(t(lang, Key::InstColEnabled));
                ui.end_row();
                for instance in &self.instance_summaries {
                    ui.label(instance.index.to_string());
                    ui.label(&instance.name);
                    ui.label(t(
                        lang,
                        if instance.running {
                            Key::CommonYes
                        } else {
                            Key::CommonNo
                        },
                    ));
                    ui.label(&instance.serial);
                    ui.label(
                        instance
                            .ldplayer_reported_port
                            .map(|port| port.to_string())
                            .unwrap_or_else(|| t(lang, Key::CommonUnknown).to_string()),
                    )
                    .on_hover_text(t(lang, Key::InstPortHover));
                    ui.label(format!(
                        "{}x{}@{}",
                        instance.width.unwrap_or(0),
                        instance.height.unwrap_or(0),
                        instance.dpi.unwrap_or(0)
                    ));
                    ui.label(
                        instance
                            .profile_id
                            .as_deref()
                            .unwrap_or_else(|| t(lang, Key::CommonNotRead)),
                    );
                    ui.label(t(
                        lang,
                        if instance.enabled {
                            Key::CommonYes
                        } else {
                            Key::CommonNo
                        },
                    ));
                    ui.end_row();
                }
            });
        });
        ui.add_space(6.0);
        ui.label(t(lang, Key::TermPortNote));
    }

    fn profiles(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        ui.heading(t(lang, Key::ProfilesHeading));
        ui.label(t(lang, Key::ProfilesIntro));
        ui.label(
            t(lang, Key::ProfilesCurrent).replace("{}", &self.selected_instance_index.to_string()),
        );
        if ui.button(t(lang, Key::ProfilesDetect)).clicked() {
            let store = self.store.clone();
            let target = self.instance_target();
            self.start_background_job(t(lang, Key::ProfilesDetectJob), move || {
                match runtime_with_target(&store, false, Some(&target)) {
                    Ok((_, device, info)) => {
                        let exact =
                            profile::find_profile(info.width, info.height, info.density_dpi);
                        match exact {
                            Some(p) => t(lang, Key::ProfilesDetectMatch)
                                .replacen("{}", &device.serial, 1)
                                .replacen("{}", &info.width.to_string(), 1)
                                .replacen("{}", &info.height.to_string(), 1)
                                .replacen("{}", &info.density_dpi.to_string(), 1)
                                .replacen("{}", p.id, 1),
                            None => t(lang, Key::ProfilesDetectNoMatch)
                                .replacen("{}", &device.serial, 1)
                                .replacen("{}", &info.width.to_string(), 1)
                                .replacen("{}", &info.height.to_string(), 1)
                                .replacen("{}", &info.density_dpi.to_string(), 1),
                        }
                    }
                    Err(e) => format!("{}: {e:#}", t(lang, Key::CommonFailed)),
                }
            });
        }
        ui.label(t(lang, Key::ProfilesNote));
        page_section(ui, lang, Key::PagePresetList);
        egui::ScrollArea::horizontal()
            .id_salt("profile_table_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(720.0);
                egui::Grid::new("profiles").striped(true).show(ui, |ui| {
                    ui.label(t(lang, Key::ProfilesColId));
                    ui.label(t(lang, Key::ProfilesColSize));
                    ui.label(t(lang, Key::ProfilesColDpi));
                    ui.label(t(lang, Key::ProfilesColOrientation));
                    ui.label(t(lang, Key::ProfilesColEligibility));
                    ui.label(t(lang, Key::ProfilesColAction));
                    ui.end_row();
                    for p in ordered_profiles() {
                        if profile::is_ldplayer_preset(p.width, p.height, p.density_dpi) {
                            ui.label(p.id);
                        } else {
                            ui.label(format!("{}{}", p.id, t(lang, Key::ProfilesHistory)));
                        }
                        ui.label(format!("{}x{}", p.width, p.height));
                        ui.label(p.density_dpi.to_string());
                        ui.label(t(
                            lang,
                            if p.orientation.as_str() == "landscape" {
                                Key::OrientationLandscape
                            } else {
                                Key::OrientationPortrait
                            },
                        ));
                        ui.label(t(
                            lang,
                            if p.calibrated {
                                Key::ProfilesCalibrated
                            } else {
                                Key::ProfilesDryOnly
                            },
                        ));
                        if ui
                            .button(format!("{}##{}", t(lang, Key::ProfilesSync), p.id))
                            .clicked()
                        {
                            self.pending_sync = Some(p.id.to_string());
                        }
                        ui.end_row();
                    }
                });
            });
    }

    fn live_confirmation_window(&mut self, ctx: &egui::Context) {
        let Some(intent) = self.pending_live_run.clone() else {
            return;
        };
        let lang = self.lang();
        let mut action = None;
        egui::Window::new(t(lang, Key::LiveTitle))
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                let scope = if intent.all_instances {
                    t(lang, Key::LiveScopeAll)
                } else {
                    t(lang, Key::LiveScopeOne)
                };
                ui.colored_label(
                    egui::Color32::YELLOW,
                    t(lang, Key::LiveScope).replace("{}", scope),
                );
                // Show the snapshot, not the current state: the user reviews
                // exactly what will run.
                ui.label(
                    t(lang, Key::LiveConfigPath)
                        .replace("{}", &intent.config_path.display().to_string()),
                );
                ui.label(
                    t(lang, Key::LiveDataRoot)
                        .replace("{}", &intent.data_root.display().to_string()),
                );
                if !intent.all_instances {
                    ui.label(
                        t(lang, Key::LiveInstanceIndex)
                            .replace("{}", &intent.instance_index.to_string()),
                    );
                }
                ui.label(t(lang, Key::LiveWarning));
                ui.label(t(lang, Key::LiveConfirmNote));
                ui.horizontal(|ui| {
                    if ui.button(t(lang, Key::LiveConfirm)).clicked() {
                        action = Some(true);
                    }
                    if ui.button(t(lang, Key::CommonCancel)).clicked() {
                        action = Some(false);
                    }
                });
            });
        if let Some(confirm) = action {
            self.pending_live_run = None;
            if confirm {
                // The dialog may have been open while the config, instance, or
                // data root changed. Never run live against a different target
                // than the one displayed.
                if self.live_intent() != intent {
                    self.status = t(lang, Key::LiveChanged).into();
                } else {
                    self.start_run(true);
                }
            } else {
                self.status = t(lang, Key::StatusCanceledLive).into();
            }
        }
    }

    fn sync_confirmation_window(&mut self, ctx: &egui::Context) {
        let Some(id) = self.pending_sync.clone() else {
            return;
        };
        let lang = self.lang();
        let mut action = None;
        egui::Window::new(t(lang, Key::SyncTitle))
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    t(lang, Key::SyncWarning).replace("{}", &id),
                );
                ui.label(t(lang, Key::SyncNote));
                ui.horizontal(|ui| {
                    if ui.button(t(lang, Key::SyncConfirm)).clicked() {
                        action = Some(true);
                    }
                    if ui.button(t(lang, Key::CommonCancel)).clicked() {
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
                self.start_background_job(t(lang, Key::SyncJob).replace("{}", &id), move || {
                    match sync_profile(&store, &sync_id, true, &target) {
                        Ok(code) => format!(
                            "{}: {}",
                            t(lang, Key::CommonFinished),
                            ui_text::exit_code_label(lang, code)
                        ),
                        Err(e) => format!("{}: {e:#}", t(lang, Key::CommonFailed)),
                    }
                });
            } else {
                self.status = t(lang, Key::StatusCanceledSync).into();
            }
        }
    }

    fn tasks_page(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        ui.heading(t(lang, Key::TasksHeading));
        ui.label(t(lang, Key::TasksIntro));
        page_section(ui, lang, Key::PageTaskSettings);
        ui.horizontal(|ui| {
            ui.label(t(lang, Key::TasksName));
            ui.text_edit_singleline(&mut self.task_name);
        });
        ui.checkbox(&mut self.task_live, t(lang, Key::TasksLive));
        ui.checkbox(
            &mut self.task_all_instances,
            t(lang, Key::TasksAllInstances),
        );
        ui.horizontal(|ui| {
            if ui.button(t(lang, Key::TasksQuery)).clicked() {
                let name = self.task_name.clone();
                let config_path = self.store.path.clone();
                self.start_background_job(t(lang, Key::TasksQuery), move || {
                    match task_command(TaskRequest::Query { name }, Some(&config_path)) {
                        Ok(code) => format!(
                            "{}: {}",
                            t(lang, Key::CommonFinished),
                            ui_text::exit_code_label(lang, code)
                        ),
                        Err(error) => format!("{}: {error:#}", t(lang, Key::CommonFailed)),
                    }
                });
            }
            if ui.button(t(lang, Key::TasksCreate)).clicked() {
                let name = self.task_name.clone();
                let live = self.task_live;
                let all_instances = self.task_all_instances;
                let config_path = self.store.path.clone();
                let target = self.instance_target();
                if live {
                    self.status = t(lang, Key::TasksLiveNeedsConfirm).into();
                } else {
                    self.start_background_job(
                        t(lang, Key::TasksCreate),
                        move || match task_command(
                            TaskRequest::Create {
                                name,
                                dry_run: true,
                                live: false,
                                confirm: false,
                                all_instances,
                                target,
                            },
                            Some(&config_path),
                        ) {
                            Ok(code) => format!(
                                "{}: {}",
                                t(lang, Key::CommonFinished),
                                ui_text::exit_code_label(lang, code)
                            ),
                            Err(error) => format!("{}: {error:#}", t(lang, Key::CommonFailed)),
                        },
                    );
                }
            }
            if ui.button(t(lang, Key::TasksDelete)).clicked() {
                let name = self.task_name.clone();
                let config_path = self.store.path.clone();
                self.start_background_job(t(lang, Key::TasksDelete), move || {
                    match task_command(
                        TaskRequest::Delete {
                            name,
                            confirm: true,
                        },
                        Some(&config_path),
                    ) {
                        Ok(code) => format!(
                            "{}: {}",
                            t(lang, Key::CommonFinished),
                            ui_text::exit_code_label(lang, code)
                        ),
                        Err(error) => format!("{}: {error:#}", t(lang, Key::CommonFailed)),
                    }
                });
            }
        });
        if self.task_live {
            ui.separator();
            ui.colored_label(egui::Color32::YELLOW, t(lang, Key::TasksLiveWarning));
            if ui.button(t(lang, Key::TasksConfirmLive)).clicked() {
                let name = self.task_name.clone();
                let all_instances = self.task_all_instances;
                let target = self.instance_target();
                let config_path = self.store.path.clone();
                self.start_background_job(
                    t(lang, Key::TasksConfirmLive),
                    move || match task_command(
                        TaskRequest::Create {
                            name,
                            dry_run: false,
                            live: true,
                            confirm: true,
                            all_instances,
                            target,
                        },
                        Some(&config_path),
                    ) {
                        Ok(code) => format!(
                            "{}: {}",
                            t(lang, Key::CommonFinished),
                            ui_text::exit_code_label(lang, code)
                        ),
                        Err(error) => format!("{}: {error:#}", t(lang, Key::CommonFailed)),
                    },
                );
                self.task_live = false;
            }
        }
        page_section(ui, lang, Key::PageTaskWindow);
        ui.label(
            t(lang, Key::TasksWindow)
                .replacen("{}", &self.store.config.window.start, 1)
                .replacen("{}", &self.store.config.window.end, 1),
        );
        ui.label(t(lang, Key::TasksGuiNote));
    }

    fn logs_page(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        ui.heading(t(lang, Key::LogsHeading));
        ui.horizontal_wrapped(|ui| {
            if ui.button(t(lang, Key::LogsRefresh)).clicked() {
                self.refresh_logs();
            }
            if ui.button(t(lang, Key::LogsOpenDir)).clicked() {
                match crate::process::open_path(&self.store.root.join("logs")) {
                    Ok(()) => self.status = t(lang, Key::StatusLogsDirOpened).into(),
                    Err(error) => {
                        self.status = format!("{}: {error:#}", t(lang, Key::CommonFailed))
                    }
                }
            }
            if ui.button(t(lang, Key::LogsExportReport)).clicked() {
                self.export_report();
            }
            if ui.button(t(lang, Key::LogsOpenReport)).clicked() {
                match self.report_path.as_ref().map(std::path::PathBuf::from) {
                    Some(path) => match crate::process::open_path(&path) {
                        Ok(()) => self.status = t(lang, Key::StatusReportOpened).into(),
                        Err(error) => {
                            self.status = format!("{}: {error:#}", t(lang, Key::CommonFailed))
                        }
                    },
                    None => self.status = t(lang, Key::StatusExportReportFirst).into(),
                }
            }
            ui.add_space(10.0);
            ui.label(t(lang, Key::LogsFilter));
            ui.add(
                egui::TextEdit::singleline(&mut self.logs_filter)
                    .hint_text(t(lang, Key::LogsFilterHint))
                    .desired_width(210.0),
            );
        });

        // Keep evidence operations outside the scrolling table so they remain
        // reachable while browsing long directories and event summaries.
        ui.horizontal_wrapped(|ui| {
            if let Some(directory) = self.selected_log_directory.clone() {
                ui.add(
                    egui::Label::new(
                        t(lang, Key::LogsSelected).replace("{}", &directory.display().to_string()),
                    )
                    .truncate(),
                );
                if ui.button(t(lang, Key::LogsOpen)).clicked() {
                    match crate::process::open_path(&directory) {
                        Ok(()) => self.status = t(lang, Key::StatusLogsDirOpened).into(),
                        Err(error) => {
                            self.status = format!("{}: {error:#}", t(lang, Key::CommonFailed))
                        }
                    }
                }
                if ui.button(t(lang, Key::LogsCopyPath)).clicked() {
                    ui.ctx().copy_text(directory.display().to_string());
                    self.status = t(lang, Key::StatusPathCopied).into();
                }
                if ui.button(t(lang, Key::LogsCopyEvidence)).clicked() {
                    if let Some(preview) = &self.selected_log_preview {
                        ui.ctx().copy_text(preview.clone());
                        self.status = t(lang, Key::StatusEvidenceCopied).into();
                    } else {
                        self.status = t(lang, Key::StatusNothingToCopy).into();
                    }
                }
                if ui.button(t(lang, Key::LogsExportSelected)).clicked() {
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
                            Ok(path) => {
                                self.status = t(lang, Key::StatusEvidenceExported)
                                    .replace("{}", &path.display().to_string())
                            }
                            Err(error) => {
                                self.status = format!("{}: {error:#}", t(lang, Key::CommonFailed))
                            }
                        }
                    }
                }
                if directory.join("events.jsonl").is_file()
                    && ui.button(t(lang, Key::LogsExportEvents)).clicked()
                    && let Some(destination) = rfd::FileDialog::new()
                        .set_file_name("events.jsonl")
                        .save_file()
                {
                    match logger::export_log_artifact_named(
                        &directory,
                        "events.jsonl",
                        &destination,
                    ) {
                        Ok(path) => {
                            self.status = t(lang, Key::StatusEventLogExported)
                                .replace("{}", &path.display().to_string())
                        }
                        Err(error) => {
                            self.status = format!("{}: {error:#}", t(lang, Key::CommonFailed))
                        }
                    }
                }
            } else {
                ui.label(t(lang, Key::LogsSelectHint));
            }
        });

        page_section(ui, lang, Key::PageRunHistory);
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
                t(lang, Key::LogsEmpty)
            } else {
                t(lang, Key::LogsNoMatch)
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
                        for heading in [
                            Key::LogsColKind,
                            Key::LogsColStatus,
                            Key::LogsColSummary,
                            Key::LogsColDir,
                        ] {
                            ui.label(egui::RichText::new(t(lang, heading)).strong());
                        }
                        ui.end_row();
                        for entry in &filtered {
                            let selected = selected_directory.as_ref() == Some(&entry.directory);
                            let response = ui.selectable_label(
                                selected,
                                ui_text::log_kind_label(lang, &entry.kind),
                            );
                            if response.clicked() || response.double_clicked() {
                                clicked_directory = Some(entry.directory.clone());
                            }
                            ui.label(ui_text::log_status_label(lang, &entry.status));
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
            page_section(ui, lang, Key::LogsPreview);
            ui.horizontal(|ui| {
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
        let lang = self.lang();
        ui.separator();
        page_anchor(ui, Key::PageEvents);
        ui.label(egui::RichText::new(t(lang, Key::LogsTimeline)).strong());
        if self.recent_events.is_empty() {
            ui.label(t(lang, Key::LogsNoEvents));
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
                        for heading in [Key::LogsColTime, Key::LogsColEvent, Key::LogsColSummary] {
                            ui.label(egui::RichText::new(t(lang, heading)).strong());
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
        let lang = self.lang();
        let mut action = None;
        egui::Window::new(t(lang, Key::ApkTitle))
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(t(lang, Key::ApkPath).replace("{}", &apk.display().to_string()));
                if let Some(metadata) = &self.pending_apk_metadata {
                    ui.label(t(lang, Key::ApkPackageName).replace("{}", &metadata.package_name));
                    ui.label(
                        t(lang, Key::ApkVersionName).replace(
                            "{}",
                            metadata
                                .version_name
                                .as_deref()
                                .unwrap_or(t(lang, Key::CommonUnknown)),
                        ),
                    );
                    ui.label(
                        t(lang, Key::ApkVersionCode).replace(
                            "{}",
                            metadata
                                .version_code
                                .as_deref()
                                .unwrap_or(t(lang, Key::CommonUnknown)),
                        ),
                    );
                }
                ui.label(
                    t(lang, Key::AppPackage).replace("{}", &self.store.config.app.package_name),
                );
                ui.colored_label(egui::Color32::YELLOW, t(lang, Key::ApkWarning));
                ui.horizontal(|ui| {
                    if ui.button(t(lang, Key::CommonConfirm)).clicked() {
                        action = Some(true);
                    }
                    if ui.button(t(lang, Key::CommonCancel)).clicked() {
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
                self.start_background_job_with_kind(
                    t(lang, Key::ApkJob),
                    JobKind::AppInstall,
                    move || match install_apk(&store, &apk, &target) {
                        Ok(result) => (
                            t(lang, Key::ApkDone)
                                .replacen("{}", &result.package_name, 1)
                                .replacen("{}", &result.version, 1)
                                .replacen("{}", result.install_output.trim(), 1),
                            Some(result.version),
                        ),
                        Err(error) => (format!("{}: {error:#}", t(lang, Key::CommonFailed)), None),
                    },
                );
            } else {
                self.status = t(lang, Key::StatusCanceledApk).into();
            }
        }
    }

    fn app_page(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        if !self.app_version_checked && self.job.is_none() {
            self.start_app_version_query();
        }
        ui.heading(t(lang, Key::AppHeading));
        ui.label(t(lang, Key::AppPackage).replace("{}", &self.store.config.app.package_name));
        ui.horizontal(|ui| {
            ui.label(t(lang, Key::AppInstalledVersion));
            ui.label(
                self.installed_version
                    .as_deref()
                    .unwrap_or_else(|| t(lang, Key::AppNotRead)),
            );
            if ui.button(t(lang, Key::AppRefreshVersion)).clicked() {
                self.start_app_version_query();
            }
        });
        page_section(ui, lang, Key::PageInstallPackage);
        ui.horizontal_wrapped(|ui| {
            ui.label(t(lang, Key::AppApkPath));
            ui.text_edit_singleline(&mut self.apk_path);
            if ui.button(t(lang, Key::AppBrowse)).clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Android APK", &["apk"])
                    .pick_file()
            {
                self.save_apk_path(path);
            }
        });
        if ui.button(t(lang, Key::AppSavePath)).clicked() {
            let path = std::path::PathBuf::from(&self.apk_path);
            self.store.config.app.apk_path = config::path_for_config(&self.store.root, &path);
            match self.store.save() {
                Ok(()) => self.status = t(lang, Key::StatusDone).into(),
                Err(e) => self.status = format!("{}: {e:#}", t(lang, Key::CommonFailed)),
            }
        }
        if ui.button(t(lang, Key::AppInstall)).clicked() {
            match config::resolve_path(&self.store.root, &self.apk_path) {
                Some(apk) if apk.is_file() => match inspect_apk_metadata(&apk) {
                    Ok(metadata) if metadata.package_name == self.store.config.app.package_name => {
                        self.pending_apk_metadata = Some(metadata);
                        self.pending_apk_install = Some(apk);
                    }
                    Ok(metadata) => {
                        self.status = t(lang, Key::AppPackageMismatch)
                            .replacen("{}", &metadata.package_name, 1)
                            .replacen("{}", &self.store.config.app.package_name, 1);
                    }
                    Err(error) => {
                        self.status = format!("{}: {error:#}", t(lang, Key::CommonFailed))
                    }
                },
                Some(apk) => {
                    self.status = t(lang, Key::AppMissing).replace("{}", &apk.display().to_string())
                }
                None => self.status = t(lang, Key::AppChooseFirst).into(),
            }
        }
        if let Some(result) = &self.apk_install_result {
            ui.separator();
            ui.label(t(lang, Key::AppLatestInstall).replace("{}", result));
        }
        ui.label(t(lang, Key::AppNote));
    }

    fn config_page(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        ui.heading(t(lang, Key::ConfigHeading));
        ui.label(t(lang, Key::ConfigIntro));
        ui.label(t(lang, Key::ConfigPath).replace("{}", &self.store.path.display().to_string()));
        for (index, heading) in ui_text::CONFIG_HEADINGS.iter().enumerate() {
            page_section(ui, lang, *heading);
            self.render_config_section(ui, lang, index);
        }
    }

    fn render_config_section(&mut self, ui: &mut egui::Ui, lang: Lang, section: usize) {
        match section {
            0 => {
                ui.horizontal(|ui| {
                    if ui.button(t(lang, Key::ConfigScan)).clicked() {
                        self.start_dependency_scan();
                    }
                    if ui.button(t(lang, Key::ConfigSaveFound)).clicked() {
                        let Some(report) = self.dependency_report.clone() else {
                            self.status = t(lang, Key::ConfigScanFirst).into();
                            return;
                        };
                        let original_ldconsole_path =
                            self.store.config.emulator.ldconsole_path.clone();
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
                                Ok(()) => self.status = t(lang, Key::ConfigPathsSaved).into(),
                                Err(error) => {
                                    self.store.config.emulator.ldconsole_path =
                                        original_ldconsole_path;
                                    self.store.config.emulator.adb_path = original_adb_path;
                                    self.status =
                                        format!("{}: {error:#}", t(lang, Key::CommonFailed));
                                }
                            }
                        } else {
                            self.status =
                                format!("{}\n{}", report.ldplayer.summary(), report.adb.summary());
                        }
                    }
                    if ui.button(t(lang, Key::ConfigOpenToml)).clicked() {
                        match crate::config_command(&self.store, ConfigRequest::Open) {
                            Ok(_) => self.status = t(lang, Key::ConfigOpened).into(),
                            Err(error) => {
                                self.status = format!("{}: {error:#}", t(lang, Key::CommonFailed))
                            }
                        }
                    }
                });
                ui.label(t(lang, Key::ConfigTomlHint));
                if let Some(report) = &self.dependency_report {
                    ui.separator();
                    ui.label(egui::RichText::new(t(lang, Key::ConfigDepsHeading)).strong());
                    Self::dependency_candidate_selector(
                        ui,
                        lang,
                        "LDPlayer",
                        &report.ldplayer,
                        &mut self.selected_ldplayer_candidate,
                    );
                    Self::dependency_candidate_selector(
                        ui,
                        lang,
                        "ADB",
                        &report.adb,
                        &mut self.selected_adb_candidate,
                    );
                }
            }
            1 => {
                ui.label(egui::RichText::new(t(lang, Key::ConfigFieldWorkHeading)).strong());
                let mut allow_field_work_live = self.store.config.runtime.allow_field_work_live;
                if ui
                    .checkbox(
                        &mut allow_field_work_live,
                        t(lang, Key::ConfigFieldWorkCheckbox),
                    )
                    .changed()
                {
                    self.store.config.runtime.allow_field_work_live = allow_field_work_live;
                    match self.store.save() {
                        Ok(()) => {
                            self.status = t(
                                lang,
                                if allow_field_work_live {
                                    Key::FieldWorkEnabled
                                } else {
                                    Key::FieldWorkDisabled
                                },
                            )
                            .into();
                        }
                        Err(error) => {
                            self.store.config.runtime.allow_field_work_live =
                                !allow_field_work_live;
                            self.status = format!("{}: {error:#}", t(lang, Key::CommonFailed));
                        }
                    }
                }
                ui.label(t(lang, Key::ConfigFieldWorkNote));
            }
            2 => {
                ui.label(egui::RichText::new(t(lang, Key::ConfigLoginHeading)).strong());
                ui.label(t(lang, Key::ConfigLoginRisk));
                let mut allow_auto_login = self.store.config.runtime.allow_auto_login;
                if ui
                    .checkbox(&mut allow_auto_login, t(lang, Key::ConfigLoginAllow))
                    .changed()
                {
                    self.store.config.runtime.allow_auto_login = allow_auto_login;
                    match self.store.save() {
                        Ok(()) => {
                            self.status = t(
                                lang,
                                if allow_auto_login {
                                    Key::AutoLoginEnabled
                                } else {
                                    Key::AutoLoginDisabled
                                },
                            )
                            .into();
                        }
                        Err(error) => {
                            self.store.config.runtime.allow_auto_login = !allow_auto_login;
                            self.status = format!("{}: {error:#}", t(lang, Key::CommonFailed));
                        }
                    }
                }
                let mut accept_login_terms = self.store.config.runtime.accept_login_terms;
                if ui
                    .checkbox(&mut accept_login_terms, t(lang, Key::ConfigLoginTerms))
                    .changed()
                {
                    self.store.config.runtime.accept_login_terms = accept_login_terms;
                    match self.store.save() {
                        Ok(()) => {
                            self.status = t(
                                lang,
                                if accept_login_terms {
                                    Key::TermsGranted
                                } else {
                                    Key::TermsRevoked
                                },
                            )
                            .into();
                        }
                        Err(error) => {
                            self.store.config.runtime.accept_login_terms = !accept_login_terms;
                            self.status = format!("{}: {error:#}", t(lang, Key::CommonFailed));
                        }
                    }
                }
                ui.horizontal(|ui| {
                    ui.label(t(lang, Key::ConfigAccountLabel));
                    let previous = self.store.config.runtime.auto_login_account.clone();
                    let mut account = previous.clone();
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut account)
                            .password(!self.show_auto_login_account)
                            .hint_text(if previous.is_empty() {
                                t(lang, Key::ConfigAccountUnset)
                            } else {
                                t(lang, Key::ConfigAccountSet)
                            }),
                    );
                    if response.changed() {
                        self.store.config.runtime.auto_login_account = account.clone();
                    }
                    if response.lost_focus() {
                        match self.store.save() {
                            Ok(()) => self.status = t(lang, Key::AccountSaved).into(),
                            Err(error) => {
                                self.store.config.runtime.auto_login_account = previous;
                                self.status = format!("{}: {error:#}", t(lang, Key::CommonFailed));
                            }
                        }
                    }
                    ui.checkbox(
                        &mut self.show_auto_login_account,
                        t(lang, Key::ConfigShowPlain),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label(t(lang, Key::ConfigLoginTimeout));
                    let mut timeout = self.store.config.runtime.auto_login_timeout_seconds;
                    if ui
                        .add(egui::DragValue::new(&mut timeout).range(5..=120))
                        .changed()
                    {
                        self.store.config.runtime.auto_login_timeout_seconds = timeout;
                        if let Err(error) = self.store.save() {
                            self.status = format!("{}: {error:#}", t(lang, Key::CommonFailed));
                        }
                    }
                });
                ui.label(t(lang, Key::ConfigLoginNote));
            }
            _ => {
                ui.label(t(lang, Key::ConfigTomlHeading));
                ui.code(
                    toml::to_string_pretty(&self.store.config.redacted_for_display())
                        .unwrap_or_default(),
                );
            }
        }
    }

    fn dependency_candidate_selector(
        ui: &mut egui::Ui,
        lang: Lang,
        label: &str,
        report: &crate::discovery::DiscoveryReport,
        selected: &mut Option<std::path::PathBuf>,
    ) {
        let selected_text = report
            .candidates
            .iter()
            .find(|candidate| candidate.exists && Some(&candidate.path) == selected.as_ref())
            .map(|candidate| format!("{} [{}]", candidate.path.display(), candidate.source))
            .unwrap_or_else(|| t(lang, Key::ConfigDepNone).to_string());
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
        ui.collapsing(
            t(lang, Key::ConfigDepCandidates).replace("{}", label),
            |ui| {
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
                    ui.label(t(lang, Key::ConfigDepOmitted).replace("{}", &omitted.to_string()));
                }
            },
        );
    }

    fn about(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        ui.heading(t(lang, Key::AboutHeading));
        ui.label(t(lang, Key::AboutVersion).replace("{}", env!("CARGO_PKG_VERSION")));
        // A dirty build is not the commit it names; surface it here so a bug
        // report cannot be attributed to a clean build of that commit (FR-014).
        match crate::git_dirty() {
            Some(true) => {
                ui.colored_label(
                    egui::Color32::from_rgb(200, 120, 0),
                    t(lang, Key::AboutDirty),
                );
            }
            Some(false) => {
                ui.label(t(lang, Key::AboutClean));
            }
            None => {
                ui.label(t(lang, Key::AboutUnknown));
            }
        }
        page_section(ui, lang, Key::AboutLanguage);
        ui.label(t(lang, Key::AboutLanguageHint));
        let mut requested = None;
        ui.horizontal(|ui| {
            for option in [Lang::Zh, Lang::En] {
                if ui
                    .selectable_label(lang == option, ui_text::language_name(option))
                    .clicked()
                {
                    requested = Some(option);
                }
            }
        });
        if let Some(option) = requested {
            self.set_language(option);
        }
        ui.collapsing(t(lang, Key::AboutTechnical), |ui| {
            ui.label(t(lang, Key::AboutGitCommit).replace("{}", crate::GIT_COMMIT));
            ui.label(t(lang, Key::AboutBuildId).replace("{}", &crate::build_id()));
        });
        page_section(ui, lang, Key::AboutLinks);
        ui.hyperlink_to(t(lang, Key::AboutLdplayerLink), "https://www.ldmnq.com/");
        ui.hyperlink_to(
            t(lang, Key::AboutAppLink),
            "https://app.fzu.edu.cn/fd-app/m/index.html",
        );
        page_section(ui, lang, Key::AboutProfiles);
        for supported in ordered_profiles() {
            let orientation = t(
                lang,
                if supported.orientation.as_str() == "landscape" {
                    Key::OrientationLandscape
                } else {
                    Key::OrientationPortrait
                },
            );
            let eligibility = t(
                lang,
                if supported.calibrated {
                    Key::ProfilesCalibrated
                } else {
                    Key::ProfilesDryOnly
                },
            );
            ui.label(format!(
                "{} {}x{}@{} {} {}",
                supported.id,
                supported.width,
                supported.height,
                supported.density_dpi,
                orientation,
                eligibility
            ));
        }
        page_section(ui, lang, Key::AboutDeps);
        ui.label(t(lang, Key::DepImage));
        ui.label(t(lang, Key::DepGui));
        ui.label(t(lang, Key::DepRfd));
        ui.label(t(lang, Key::DepSerde));
        ui.label(t(lang, Key::DepExternal));
        ui.separator();
        ui.label(t(lang, Key::AboutAdvice));
    }
}

/// The "what do I need to install" checklist, shared by the Quick-start page
/// and the Setup page so both cannot drift apart.
fn render_environment_checklist(ui: &mut egui::Ui, lang: Lang) {
    egui::Grid::new("environment_checklist")
        .striped(true)
        .min_col_width(120.0)
        .show(ui, |ui| {
            ui.label(egui::RichText::new(t(lang, Key::EnvColNeed)).strong());
            ui.label(egui::RichText::new(t(lang, Key::EnvColRequired)).strong());
            ui.label(egui::RichText::new(t(lang, Key::EnvColSource)).strong());
            ui.end_row();
            for (need, required, source) in [
                (Key::EnvWindows, Key::CommonRequired, Key::EnvWindowsSrc),
                (Key::EnvLdplayer, Key::CommonRequired, Key::EnvLdplayerSrc),
                (Key::EnvExe, Key::CommonRequired, Key::EnvExeSrc),
                (Key::EnvApk, Key::CommonRequired, Key::EnvApkSrc),
                (Key::EnvAdb, Key::CommonNotRequired, Key::EnvAdbSrc),
                (Key::EnvRoot, Key::CommonNotRequired, Key::EnvRootSrc),
            ] {
                ui.label(t(lang, need));
                ui.label(t(lang, required));
                ui.label(t(lang, source));
                ui.end_row();
            }
        });
}

/// Short "does / does not" columns replacing the long safety paragraphs.
fn render_safety_table(ui: &mut egui::Ui, lang: Lang) {
    ui.label(egui::RichText::new(t(lang, Key::SafetyTitle)).strong());
    egui::Grid::new("safety_table")
        .striped(true)
        .min_col_width(200.0)
        .show(ui, |ui| {
            ui.label(egui::RichText::new(t(lang, Key::SafetyDoTitle)).strong());
            ui.label(egui::RichText::new(t(lang, Key::SafetyDontTitle)).strong());
            ui.end_row();
            for (does, does_not) in [
                (Key::SafetyDoLaunch, Key::SafetyDontCredentials),
                (Key::SafetyDoTap, Key::SafetyDontPopups),
                (Key::SafetyDoLogin, Key::SafetyDontStore),
                (Key::SafetyDoWrite, Key::SafetyDontUpload),
            ] {
                ui.label(t(lang, does));
                ui.label(t(lang, does_not));
                ui.end_row();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    fn configured_store() -> ConfigStore {
        let root = std::env::temp_dir().join(format!(
            "zhfd-gui-harness-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut config = config::Config::default();
        config.emulator.ldconsole_path = "tools/ldconsole.exe".into();
        config.emulator.adb_path = "tools/adb.exe".into();
        ConfigStore {
            root: root.clone(),
            path: root.join("config.toml"),
            config,
            origin: config::ConfigOrigin::Explicit,
            created_default: false,
            recovery_error: None,
        }
    }

    fn gui_harness(size: egui::Vec2) -> Harness<'static, GuiApp> {
        let store = configured_store();
        Harness::builder()
            .with_size(size)
            .with_max_steps(2)
            .build_eframe(|cc| {
                configure_fonts(&cc.egui_ctx);
                GuiApp::new(store, None)
            })
    }

    fn assert_all_nodes_fit(harness: &Harness<'static, GuiApp>, label: &str, width: f32) {
        let nodes: Vec<_> = harness.query_all_by_label(label).collect();
        assert!(!nodes.is_empty(), "no visible node matched {label:?}");
        assert!(
            nodes.iter().all(|node| node.rect().max.x <= width),
            "node {label:?} extends beyond width {width}"
        );
    }

    fn assert_matching_nodes_fit(harness: &Harness<'static, GuiApp>, text: &str, width: f32) {
        let nodes: Vec<_> = harness.query_all_by_label_contains(text).collect();
        assert!(!nodes.is_empty(), "no visible node contained {text:?}");
        assert!(
            nodes.iter().all(|node| node.rect().max.x <= width),
            "node containing {text:?} extends beyond width {width}"
        );
    }

    #[test]
    fn gui_copy_has_no_hard_coded_machine_fields() {
        // The banned strings are assembled from parts so this test's own
        // source does not contain them.
        let source = include_str!("gui.rs");
        let banned = [
            concat!("status", "={"),
            concat!("route", "={"),
            concat!("error_code", "={"),
            concat!("退出", "码 {:?}"),
            concat!("暂无 ", "result.json"),
            concat!("首次设置", "(Setup)"),
            concat!("Rust CLI/GUI", " 迁移第一阶段"),
        ];
        for value in banned {
            assert!(
                !source.contains(value),
                "gui.rs still hard-codes user-visible machine output: {value}"
            );
        }
    }

    #[test]
    fn quick_use_is_a_single_column_user_flow_at_narrow_width() {
        let mut harness = gui_harness(egui::vec2(800.0, 700.0));
        harness.get_by_label(t(Lang::Zh, Key::NavQuickUse)).click();
        harness.run_steps(1);

        for heading in [
            Key::QuickPrepareTitle,
            Key::QuickSetupTitle,
            Key::QuickRunTitle,
            Key::QuickIssuesTitle,
            Key::QuickAdvancedTitle,
        ] {
            assert_all_nodes_fit(&harness, t(Lang::Zh, heading), 800.0);
        }
        assert_all_nodes_fit(&harness, t(Lang::Zh, Key::SafetyTitle), 800.0);

        harness
            .get_by_label(t(Lang::Zh, Key::QuickIssuesTitle))
            .click_accesskit();
        harness.run_steps(2);
        harness
            .get_by_label(t(Lang::Zh, Key::QuickIssuesTitle))
            .scroll_to_me();
        harness.run_steps(2);
        assert_matching_nodes_fit(&harness, "设备没连上", 800.0);
    }

    #[test]
    fn config_page_shows_all_sections_in_one_wrapping_column() {
        let mut harness = gui_harness(egui::vec2(800.0, 700.0));
        harness.get_by_label(t(Lang::Zh, Key::NavConfig)).click();
        harness.run_steps(1);

        for heading in ui_text::CONFIG_HEADINGS {
            assert_all_nodes_fit(&harness, t(Lang::Zh, heading), 800.0);
        }
        assert_all_nodes_fit(&harness, t(Lang::Zh, Key::ConfigOpenToml), 800.0);
    }

    #[test]
    fn page_contents_jump_to_configuration_sections_in_wide_and_narrow_windows() {
        use egui::accesskit::Role;

        for width in [800.0, 1280.0] {
            let mut harness = gui_harness(egui::vec2(width, 500.0));
            harness.get_by_label("配置文件").click();
            harness.run_steps(3);
            if width == 800.0 {
                harness.get_by_label("本页目录").click();
                harness.run_steps(3);
            }
            let open_button_before = harness.get_by_label("打开 config.toml").rect();
            let destination = "当前配置内容";
            let before = harness
                .get_by_role_and_label(Role::Label, destination)
                .rect();
            harness
                .get_by_role_and_label(Role::Button, destination)
                .click();
            harness.run_steps(8);
            let after = harness
                .get_by_role_and_label(Role::Label, destination)
                .rect();
            assert!(after.min.y < before.min.y && after.max.y < 475.0);
            assert!((after.min.x - before.min.x).abs() < 1.0);
            assert!(open_button_before.max.x <= width);
        }
    }

    #[test]
    fn unconfirmed_checkin_page_shows_a_specific_no_tap_message() {
        let store = configured_store();
        let log = logger::RunLog::new(&store.root).unwrap();
        let mut result = crate::new_run_result(
            crate::domain::Mode::DryRun,
            &log,
            &InstanceTargetArgs::default(),
        );
        result.error_code = Some("route_candidate_unconfirmed".into());
        result.page_route = Some(crate::domain::PageRoute::RouteCandidateUnconfirmed);
        let mut harness = Harness::builder()
            .with_size(egui::vec2(800.0, 700.0))
            .build_eframe(|cc| {
                configure_fonts(&cc.egui_ctx);
                let mut app = GuiApp::new(store, None);
                app.latest_result = Some(result);
                app
            });
        harness.run_steps(2);
        for words in ["无法确认签到页面", "无晚点名清单", "未点击签到"] {
            assert_matching_nodes_fit(&harness, words, 800.0);
        }
    }

    #[test]
    fn about_language_switch_changes_visible_navigation_copy() {
        let mut harness = gui_harness(egui::vec2(1000.0, 700.0));
        harness.get_by_label(t(Lang::Zh, Key::NavAbout)).click();
        harness.run_steps(2);
        harness.get_by_label("English").click();
        harness.run_steps(2);

        assert_all_nodes_fit(&harness, t(Lang::En, Key::AboutHeading), 1000.0);
        assert_all_nodes_fit(&harness, t(Lang::En, Key::NavQuickUse), 1000.0);
    }

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

    #[test]
    fn requires_setup_tracks_dependencies_and_recovery_state() {
        let root = std::path::PathBuf::from("C:/tmp/zhfd-gui-test");
        let store = |config: config::Config,
                     created_default: bool,
                     recovery_error: Option<String>| ConfigStore {
            root: root.clone(),
            path: root.join("config.toml"),
            config,
            origin: config::ConfigOrigin::Explicit,
            created_default,
            recovery_error,
        };

        // Embedded defaults have empty tool paths.
        assert!(requires_setup(&store(
            config::Config::default(),
            false,
            None
        )));
        // A created-default config keeps Setup open even after a write.
        assert!(requires_setup(&store(
            config::Config::default(),
            true,
            None
        )));
        // A damaged config held in recovery mode is never "ready".
        assert!(requires_setup(&store(
            config::Config::default(),
            false,
            Some("parse error".into())
        )));

        let mut filled = config::Config::default();
        filled.emulator.ldconsole_path = "E:/leidian/ldconsole.exe".into();
        filled.emulator.adb_path = "E:/leidian/adb.exe".into();
        assert!(!requires_setup(&store(filled.clone(), false, None)));

        // A created-default flag still wins, and one blank path keeps the gate
        // closed even when the other path is set.
        assert!(requires_setup(&store(filled.clone(), true, None)));
        let mut half = filled;
        half.emulator.adb_path = "   ".into();
        assert!(requires_setup(&store(half, false, None)));
    }
}
