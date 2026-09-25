use crate::config::ConfigStore;
use crate::domain::RunResult;
use crate::{ConfigCommand, RunArgs, app_command, diagnose, run, runtime, sync_profile};
use crate::{config, logger, profile};
use eframe::egui;
use std::sync::{Arc, Mutex};
use std::thread;

pub fn launch(store: ConfigStore) -> anyhow::Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "智汇福大自动签到",
        options,
        Box::new(move |_cc| Ok(Box::new(GuiApp::new(store)))),
    )
    .map_err(|e| anyhow::anyhow!("GUI 启动失败: {}", e))
}

struct GuiApp {
    store: ConfigStore,
    tab: usize,
    status: String,
    apk_path: String,
    job: Option<Arc<Mutex<Option<String>>>>,
    pending_sync: Option<String>,
    latest_result: Option<RunResult>,
    latest_log_dir: Option<String>,
    report_path: Option<String>,
}

impl GuiApp {
    fn new(store: ConfigStore) -> Self {
        let apk_path = store.config.app.apk_path.clone();
        Self {
            store,
            tab: 0,
            status: "就绪。默认模式为 dry-run。".into(),
            apk_path,
            job: None,
            pending_sync: None,
            latest_result: None,
            latest_log_dir: None,
            report_path: None,
        }
    }

    fn start_dry_run(&mut self) {
        if self.job.is_some() {
            self.status = "已有任务运行中".into();
            return;
        }
        let slot = Arc::new(Mutex::new(None));
        let result_slot = slot.clone();
        let store = self.store.clone();
        thread::spawn(move || {
            let result = match run(
                &store,
                RunArgs {
                    dry_run: true,
                    live: false,
                    confirm: false,
                },
            ) {
                Ok(code) => format!("dry-run 完成，退出码 {:?}", code),
                Err(e) => format!("dry-run 失败: {:#}", e),
            };
            *result_slot.lock().unwrap() = Some(result);
        });
        self.job = Some(slot);
        self.status = "dry-run 已启动，正在等待结果……".into();
    }

    fn refresh_latest_result(&mut self) {
        match logger::latest_result(&self.store.root) {
            Ok(Some((dir, result))) => {
                self.latest_log_dir = Some(dir.display().to_string());
                self.latest_result = Some(result);
            }
            Ok(None) => {
                self.latest_log_dir = None;
                self.latest_result = None;
            }
            Err(error) => self.status = format!("读取运行结果失败: {:#}", error),
        }
    }

    fn poll_job(&mut self) {
        let result = self
            .job
            .as_ref()
            .and_then(|slot| slot.lock().ok().and_then(|mut guard| guard.take()));
        if let Some(result) = result {
            self.status = result;
            self.job = None;
            self.refresh_latest_result();
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
                for (i, label) in ["首页", "诊断", "Profile", "应用", "配置", "帮助/About"]
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
        });
        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            0 => self.home(ui),
            1 => self.diagnostics(ui),
            2 => self.profiles(ui),
            3 => self.app_page(ui),
            4 => self.config_page(ui),
            _ => self.about(ui),
        });
    }
}

impl GuiApp {
    fn home(&mut self, ui: &mut egui::Ui) {
        ui.heading("运行控制");
        ui.label("Rust CLI/GUI 迁移第一阶段：默认只进行 dry-run，不执行真实点击。");
        ui.horizontal(|ui| {
            if ui.button("环境诊断").clicked() {
                self.tab = 1;
            }
            if ui.button("运行 dry-run").clicked() {
                self.start_dry_run();
            }
            if ui.button("打开配置").clicked() {
                self.tab = 4;
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
                match diagnose(&self.store) {
                    Ok(code) => self.status = format!("诊断完成，退出码 {:?}", code),
                    Err(e) => self.status = format!("诊断失败: {:#}", e),
                }
            }
            if ui.button("导出诊断报告").clicked() {
                self.export_report();
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

    fn profiles(&mut self, ui: &mut egui::Ui) {
        ui.heading("内置 Profile");
        if ui.button("检测当前模拟器 Profile").clicked() {
            match runtime(&self.store, false) {
                Ok((_, device, info)) => {
                    let exact = profile::find_profile(info.width, info.height, info.density_dpi);
                    self.status = match exact {
                        Some(p) => format!(
                            "当前 {} {}x{}@{}，匹配 {}",
                            device.serial, info.width, info.height, info.density_dpi, p.id
                        ),
                        None => format!(
                            "当前 {} {}x{}@{}，没有精确匹配 profile",
                            device.serial, info.width, info.height, info.density_dpi
                        ),
                    };
                }
                Err(e) => self.status = format!("Profile 检测失败: {:#}", e),
            }
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
        if let Some(id) = self.pending_sync.clone() {
            ui.separator();
            ui.colored_label(
                egui::Color32::YELLOW,
                format!("将修改 LDPlayer 分辨率/DPI并重启实例：{}", id),
            );
            if ui.button("再次点击确认同步").clicked() {
                match sync_profile(&self.store, &id, true) {
                    Ok(code) => self.status = format!("同步完成，退出码 {:?}", code),
                    Err(e) => self.status = format!("同步失败: {:#}", e),
                }
                self.pending_sync = None;
            }
            if ui.button("取消同步").clicked() {
                self.pending_sync = None;
            }
        }
    }

    fn app_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("应用管理");
        ui.label(format!("目标包名：{}", self.store.config.app.package_name));
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
            let path = config::resolve_path(&self.store.root, &self.apk_path);
            match path {
                Some(apk) => match app_command(&self.store, crate::AppCommand::Install { apk }) {
                    Ok(code) => self.status = format!("安装完成，退出码 {:?}", code),
                    Err(e) => self.status = format!("安装失败: {:#}", e),
                },
                None => self.status = "请先选择 APK".into(),
            }
        }
        ui.label("当前阶段支持标准 .apk；不自动下载、不修改账号或设备身份。");
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
        ui.hyperlink_to("雷电模拟器官网", "https://www.ldmnq.com/");
        ui.hyperlink_to("智汇福大官网", "https://app.fzu.edu.cn/fd-app/m/index.html");
        ui.separator();
        ui.label("使用建议：先执行诊断，再运行 dry-run；正式模式需要显式确认。");
    }
}
