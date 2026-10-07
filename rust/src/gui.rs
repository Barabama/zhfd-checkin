use crate::config::ConfigStore;
use crate::domain::RunResult;
use crate::{
    ConfigCommand, InstanceTargetArgs, RunArgs, TaskCommand, analyze_image_file, diagnose,
    dump_ui_hierarchy, inspect_apk_metadata, install_apk, installed_app_version,
    list_instance_summaries, run, runtime_with_target, sync_profile, task_command,
};
use crate::{config, logger, profile};
use eframe::egui;
use std::sync::{Arc, Mutex};
use std::thread;

pub fn launch(store: ConfigStore) -> anyhow::Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "智汇福大自动签到",
        options,
        Box::new(move |cc| {
            configure_fonts(&cc.egui_ctx);
            Ok(Box::new(GuiApp::new(store)))
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
    apk_path: String,
    job: Option<GuiJob>,
    pending_sync: Option<String>,
    pending_live_run: bool,
    latest_result: Option<RunResult>,
    latest_log_dir: Option<String>,
    report_path: Option<String>,
    pending_apk_install: Option<std::path::PathBuf>,
    pending_apk_metadata: Option<crate::apk::ApkMetadata>,
    installed_version: Option<String>,
    app_version_checked: bool,
    apk_install_result: Option<String>,
    recent_logs: Vec<logger::LogSummary>,
    task_name: String,
    task_live: bool,
    task_all_instances: bool,
    run_all_instances: bool,
    instance_summaries: Vec<crate::InstanceSummary>,
    selected_instance_index: u32,
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
    fn new(store: ConfigStore) -> Self {
        let apk_path = store.config.app.apk_path.clone();
        let selected_instance_index = store
            .config
            .emulator
            .effective_instances()
            .first()
            .map(|instance| instance.instance_index)
            .unwrap_or(store.config.emulator.instance_index);
        Self {
            store,
            tab: 0,
            status: "就绪。默认模式为 dry-run。".into(),
            apk_path,
            job: None,
            pending_sync: None,
            pending_live_run: false,
            latest_result: None,
            latest_log_dir: None,
            report_path: None,
            pending_apk_install: None,
            pending_apk_metadata: None,
            installed_version: None,
            app_version_checked: false,
            apk_install_result: None,
            recent_logs: Vec::new(),
            task_name: "ZHFD-AutoCheckin".into(),
            task_live: false,
            task_all_instances: false,
            run_all_instances: false,
            instance_summaries: Vec::new(),
            selected_instance_index,
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
        match logger::recent_logs(&self.store.root, 12) {
            Ok(logs) => self.recent_logs = logs,
            Err(error) => self.status = format!("读取运行日志失败: {:#}", error),
        }
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

fn explain_error(error: &str) -> &'static str {
    match error {
        "unknown_state" => "视觉状态未能确认；请查看最新日志截图。",
        "location_timeout" => "定位等待超时；未执行点击。",
        "success_timeout" => "点击后未在超时内确认成功；请人工核对页面。",
        "outside_window" => "当前不在配置的签到时间窗口；未执行点击。",
        "foreground_package_changed" => "前台应用已变化；安全策略阻止点击。",
        "ready_confirmation_failed" => "点击前二次 ready 确认失败；未执行点击。",
        "unknown_profile" => "设备分辨率/DPI 没有精确 profile；未执行点击。",
        "uncalibrated_profile" => "当前 profile 尚未完成生产标定；未执行点击。",
        "live_confirmation_required" => "正式模式需要同时指定 --live --confirm；未执行点击。",
        other if other.contains("ADB") => "ADB 设备不可用；请检查模拟器和 serial。",
        _ => "请查看诊断报告和运行日志。",
    }
}

impl eframe::App for GuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_job();
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.heading("智汇福大自动签到");
            ui.horizontal(|ui| {
                for (i, label) in [
                    "首页",
                    "诊断",
                    "实例",
                    "Profile",
                    "应用",
                    "配置",
                    "计划任务",
                    "运行日志",
                    "帮助/About",
                ]
                .iter()
                .enumerate()
                {
                    if ui.selectable_label(self.tab == i, *label).clicked() {
                        self.tab = i;
                    }
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
            _ => self.about(ui),
        });
        self.sync_confirmation_window(ctx);
        self.apk_install_confirmation_window(ctx);
        self.live_confirmation_window(ctx);
    }
}

impl GuiApp {
    fn home(&mut self, ui: &mut egui::Ui) {
        ui.heading("运行控制");
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
                self.start_dry_run();
            }
            if ui.button("准备 live 运行").clicked() {
                self.pending_live_run = true;
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
                if result.success {
                    "success"
                } else {
                    result.error.as_deref().unwrap_or("未完成")
                }
            ));
        }
    }

    fn diagnostics(&mut self, ui: &mut egui::Ui) {
        ui.heading("环境诊断");
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
                    "点击：{}",
                    if result.clicked { "是" } else { "否" }
                ));
                ui.label(format!(
                    "成功：{}",
                    if result.success { "是" } else { "否" }
                ));
            });
            if let Some(error) = &result.error {
                ui.colored_label(
                    egui::Color32::LIGHT_RED,
                    format!("错误：{}（{}）", error, explain_error(error)),
                );
            } else {
                ui.colored_label(egui::Color32::LIGHT_GREEN, "未记录运行错误");
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
        ui.label("当前阶段：默认 dry-run；正式点击必须使用 CLI 的 --live --confirm，并通过 profile 标定和时间窗口门禁。");
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
            ui.label("serial");
            ui.label("尺寸/DPI");
            ui.label("启用");
            ui.end_row();
            for instance in &self.instance_summaries {
                ui.label(instance.index.to_string());
                ui.label(&instance.name);
                ui.label(if instance.running { "是" } else { "否" });
                ui.label(&instance.serial);
                ui.label(format!(
                    "{}x{}@{}",
                    instance.width.unwrap_or(0),
                    instance.height.unwrap_or(0),
                    instance.dpi.unwrap_or(0)
                ));
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
        egui::Grid::new("profiles").striped(true).show(ui, |ui| {
            ui.label("ID");
            ui.label("尺寸");
            ui.label("DPI");
            ui.label("方向");
            ui.label("标定");
            ui.label("操作");
            ui.end_row();
            for p in profile::PROFILES {
                ui.label(p.id);
                ui.label(format!("{}x{}", p.width, p.height));
                ui.label(p.density_dpi.to_string());
                ui.label(p.orientation.as_str());
                ui.label(if p.calibrated { "是" } else { "否" });
                if ui.button(format!("准备同步##{}", p.id)).clicked() {
                    self.pending_sync = Some(p.id.to_string());
                }
                ui.end_row();
            }
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
                self.start_background_job("计划任务查询", move || {
                    match task_command(TaskCommand::Query { name }) {
                        Ok(code) => format!("完成，退出码 {:?}", code),
                        Err(error) => format!("失败: {:#}", error),
                    }
                });
            }
            if ui.button("创建任务").clicked() {
                let name = self.task_name.clone();
                let live = self.task_live;
                let all_instances = self.task_all_instances;
                let target = self.instance_target();
                if live {
                    self.status = "live 计划任务需要再次确认；请点击下方确认按钮".into();
                } else {
                    self.start_background_job("创建 dry-run 计划任务", move || {
                        match task_command(TaskCommand::Create {
                            name,
                            live: false,
                            confirm: false,
                            all_instances,
                            target,
                        }) {
                            Ok(code) => format!("完成，退出码 {:?}", code),
                            Err(error) => format!("失败: {:#}", error),
                        }
                    });
                }
            }
            if ui.button("删除任务").clicked() {
                let name = self.task_name.clone();
                self.start_background_job("删除计划任务", move || {
                    match task_command(TaskCommand::Delete { name }) {
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
                self.start_background_job("创建 live 计划任务", move || {
                    match task_command(TaskCommand::Create {
                        name,
                        live: true,
                        confirm: true,
                        all_instances,
                        target,
                    }) {
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
        ui.heading("运行日志");
        if ui.button("刷新日志列表").clicked() {
            self.refresh_logs();
        }
        if self.recent_logs.is_empty() {
            ui.label("尚未找到运行或同步日志。");
            return;
        }
        egui::Grid::new("recent_logs").striped(true).show(ui, |ui| {
            ui.label("类型");
            ui.label("状态");
            ui.label("摘要");
            ui.label("目录");
            ui.end_row();
            for log in &self.recent_logs {
                ui.label(&log.kind);
                ui.label(&log.status);
                ui.label(&log.summary);
                ui.label(log.directory.display().to_string());
                ui.end_row();
            }
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
        ui.label(format!(
            "配置文件位于 EXE 根目录：{}",
            self.store.path.display()
        ));
        if ui.button("用系统默认编辑器打开 config.toml").clicked() {
            match crate::config_command(&self.store, ConfigCommand::Open) {
                Ok(_) => self.status = "已打开配置文件".into(),
                Err(e) => self.status = format!("打开失败: {:#}", e),
            }
        }
        ui.code(toml::to_string_pretty(&self.store.config).unwrap_or_default());
    }

    fn about(&mut self, ui: &mut egui::Ui) {
        ui.heading("帮助 / About");
        ui.label(format!("版本：{}", env!("CARGO_PKG_VERSION")));
        ui.label(format!("Git commit：{}", crate::GIT_COMMIT));
        ui.hyperlink_to("雷电模拟器官网", "https://www.ldmnq.com/");
        ui.hyperlink_to("智汇福大官网", "https://app.fzu.edu.cn/fd-app/m/index.html");
        ui.separator();
        ui.label("支持的 Profile：");
        for supported in profile::PROFILES {
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
