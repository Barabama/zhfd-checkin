mod adb;
mod apk;
mod config;
mod discovery;
mod domain;
mod gui;
mod ldplayer;
mod logger;
mod process;
mod profile;
mod vision;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use config::ConfigStore;
use domain::{CheckinMode, CheckinState, ConsecutiveUnknown, ExitCode, Mode, PageRoute, RunResult};
use logger::{RunLog, SyncGeometry, SyncLog, SyncResult};
use std::{
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

#[allow(dead_code)]
const LDPLAYER_URL: &str = "https://www.ldmnq.com/";
#[allow(dead_code)]
const ZHFD_URL: &str = "https://app.fzu.edu.cn/fd-app/m/index.html";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const GIT_COMMIT: &str = match option_env!("ZHFD_GIT_COMMIT") {
    Some(value) => value,
    None => "unknown",
};

#[derive(Parser, Debug)]
#[command(name="zhfd-checkin", version=VERSION, about="智汇福大 LDPlayer 自动签到控制器")]
struct Cli {
    #[command(subcommand)]
    command: Option<CommandKind>,
}

#[derive(Subcommand, Debug)]
enum CommandKind {
    Diagnose {
        #[command(flatten)]
        target: InstanceTargetArgs,
    },
    Report,
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    Run(RunArgs),
    App {
        #[command(subcommand)]
        command: AppCommand,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    About,
    Gui,
    Instance {
        #[command(subcommand)]
        command: InstanceCommand,
    },
    Task {
        #[command(subcommand)]
        command: TaskCommand,
    },
    Ui {
        #[command(subcommand)]
        command: UiCommand,
    },
    Vision {
        #[arg(long)]
        image: PathBuf,
    },
}
#[derive(Subcommand, Debug)]
enum ProfileCommand {
    List,
    Detect {
        #[command(flatten)]
        target: InstanceTargetArgs,
    },
    Sync {
        #[arg(long)]
        id: String,
        #[arg(long)]
        confirm: bool,
        #[command(flatten)]
        target: InstanceTargetArgs,
    },
}
#[derive(Subcommand, Debug)]
enum AppCommand {
    Install {
        #[arg(long)]
        apk: PathBuf,
        #[command(flatten)]
        target: InstanceTargetArgs,
    },
}
#[derive(Subcommand, Debug)]
enum ConfigCommand {
    Show,
    Open,
}
#[derive(Subcommand, Debug)]
enum InstanceCommand {
    List,
}

#[derive(Args, Debug, Clone, Default)]
struct InstanceTargetArgs {
    #[arg(long, help = "选择 LDPlayer 实例 index；默认使用 config.toml")]
    instance_index: Option<u32>,
    #[arg(long, help = "显式指定 ADB serial；覆盖实例 index 推导值")]
    serial: Option<String>,
}

#[derive(Subcommand, Debug)]
enum TaskCommand {
    Create {
        #[arg(long, default_value = "ZHFD-AutoCheckin")]
        name: String,
        #[arg(long)]
        live: bool,
        #[arg(long, requires = "live")]
        confirm: bool,
        #[arg(long, help = "让计划任务依次运行所有已发现实例")]
        all_instances: bool,
        #[command(flatten)]
        target: InstanceTargetArgs,
    },
    Delete {
        #[arg(long, default_value = "ZHFD-AutoCheckin")]
        name: String,
    },
    Query {
        #[arg(long, default_value = "ZHFD-AutoCheckin")]
        name: String,
    },
}
#[derive(Subcommand, Debug)]
enum UiCommand {
    Dump {
        #[command(flatten)]
        target: InstanceTargetArgs,
    },
}
#[derive(Args, Debug, Clone)]
struct RunArgs {
    #[arg(long, conflicts_with = "live")]
    dry_run: bool,
    #[arg(long)]
    live: bool,
    #[arg(long, requires = "live")]
    confirm: bool,
    #[arg(long, help = "依次运行所有已发现的 LDPlayer 实例")]
    all_instances: bool,
    #[command(flatten)]
    target: InstanceTargetArgs,
}

#[allow(dead_code)]
fn main() {
    let code = match run_cli() {
        Ok(code) => code as i32,
        Err(error) => {
            eprintln!("错误: {:#}", error);
            ExitCode::ConfigFailure as i32
        }
    };
    std::process::exit(code);
}

#[allow(dead_code)]
fn run_cli() -> Result<ExitCode> {
    let cli = Cli::parse();
    let store = ConfigStore::load()?;
    match cli.command.unwrap_or(CommandKind::Gui) {
        CommandKind::About => {
            print_about();
            Ok(ExitCode::Ok)
        }
        CommandKind::Gui => launch_gui_companion(),
        CommandKind::Instance { command } => instance_command(&store, command),
        CommandKind::Task { command } => task_command(command),
        CommandKind::Ui { command } => ui_command(&store, command),
        CommandKind::Vision { image } => analyze_image(&image),
        CommandKind::Config { command } => config_command(&store, command),
        CommandKind::Profile { command } => profile_command(&store, command),
        CommandKind::Diagnose { target } => diagnose(&store, &target),
        CommandKind::Report => write_report(&store),
        CommandKind::App { command } => app_command(&store, command),
        CommandKind::Run(args) => run(&store, args),
    }
}

#[derive(Debug, Clone)]
struct ResolvedInstance {
    index: u32,
    name: String,
    serial: String,
}

fn resolve_instance_target(
    ld: &ldplayer::LdPlayer,
    store: &ConfigStore,
    target: Option<&InstanceTargetArgs>,
) -> Result<ResolvedInstance> {
    let target = target.cloned().unwrap_or_default();
    let instances = ld.instances()?;
    if instances.is_empty() {
        bail!("LDPlayer 没有可用实例");
    }
    let configured_instances = store.config.emulator.effective_instances();
    let requested_serial = target.serial.filter(|value| !value.trim().is_empty());

    let index_from_serial = requested_serial.as_deref().and_then(|serial| {
        instances.iter().find_map(|instance| {
            let default_serial = ldplayer::LdPlayer::tcp_serial_for_index(instance.index)?;
            let configured_match = configured_instances.iter().any(|configured| {
                configured.instance_index == instance.index && configured.serial.trim() == serial
            });
            (default_serial == serial || configured_match).then_some(instance.index)
        })
    });

    if requested_serial.is_some() && index_from_serial.is_none() {
        bail!(
            "ADB serial 未匹配到已发现的 LDPlayer 实例: {}",
            requested_serial.as_deref().unwrap()
        );
    }
    if let (Some(requested_index), Some(mapped_index)) = (target.instance_index, index_from_serial)
        && requested_index != mapped_index
    {
        bail!(
            "实例 index 与 ADB serial 不一致: index={} serial={} 实际对应 index={}",
            requested_index,
            requested_serial.as_deref().unwrap_or(""),
            mapped_index
        );
    }

    let index = target
        .instance_index
        .or(index_from_serial)
        .unwrap_or(store.config.emulator.instance_index);
    let instance = instances
        .iter()
        .find(|instance| instance.index == index)
        .with_context(|| format!("LDPlayer 实例不存在: {}", index))?;
    let configured_instance = configured_instances
        .into_iter()
        .find(|configured| configured.instance_index == index);
    let serial = requested_serial
        .or_else(|| {
            configured_instance
                .as_ref()
                .filter(|configured| !configured.serial.trim().is_empty())
                .map(|configured| configured.serial.clone())
        })
        .or_else(|| {
            let configured = store.config.emulator.serial.trim();
            (index == store.config.emulator.instance_index && !configured.is_empty())
                .then(|| configured.to_string())
        })
        .or(ldplayer::LdPlayer::tcp_serial_for_index(index))
        .with_context(|| format!("LDPlayer 实例 index 超出 TCP ADB 端口范围: {index}"))?;
    let name = configured_instance
        .as_ref()
        .filter(|configured| !configured.name.trim().is_empty())
        .map(|configured| configured.name.clone())
        .unwrap_or_else(|| instance.name.clone());
    Ok(ResolvedInstance {
        index,
        name,
        serial,
    })
}

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct InstanceSummary {
    pub index: u32,
    pub name: String,
    pub running: bool,
    pub serial: String,
    pub ldplayer_reported_port: Option<u16>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub dpi: Option<u32>,
    pub configured: bool,
    pub enabled: bool,
}

pub(crate) fn list_instance_summaries(store: &ConfigStore) -> Result<Vec<InstanceSummary>> {
    let ld = ldplayer::LdPlayer::discover(
        &configured_path(store, &store.config.emulator.ldconsole_path),
        &store.root,
    )?;
    let configured_instances = store.config.emulator.effective_instances();
    ld.instances()?
        .into_iter()
        .map(|instance| -> Result<InstanceSummary> {
            let configured = configured_instances
                .iter()
                .find(|configured| configured.instance_index == instance.index);
            let serial =
                match configured
                    .filter(|configured| !configured.serial.trim().is_empty())
                    .map(|configured| configured.serial.clone())
                {
                    Some(serial) => serial,
                    None => ldplayer::LdPlayer::tcp_serial_for_index(instance.index).with_context(
                        || format!("实例 index 超出 TCP ADB 端口范围: {}", instance.index),
                    )?,
                };
            Ok(InstanceSummary {
                index: instance.index,
                name: configured
                    .filter(|item| !item.name.trim().is_empty())
                    .map(|item| item.name.clone())
                    .unwrap_or(instance.name),
                running: instance.running,
                serial,
                ldplayer_reported_port: instance.ldplayer_reported_port,
                width: instance.width,
                height: instance.height,
                dpi: instance.dpi,
                configured: configured.is_some(),
                enabled: configured.is_some_and(|item| item.enabled),
            })
        })
        .collect()
}

fn instance_command(store: &ConfigStore, command: InstanceCommand) -> Result<ExitCode> {
    match command {
        InstanceCommand::List => {
            for instance in list_instance_summaries(store)? {
                println!(
                    "index={} name={} running={} serial={} ldplayer_reported_port={:?} size={:?}x{:?} dpi={:?} configured={} enabled={}",
                    instance.index,
                    instance.name,
                    instance.running,
                    instance.serial,
                    instance.ldplayer_reported_port,
                    instance.width,
                    instance.height,
                    instance.dpi,
                    instance.configured,
                    instance.enabled,
                );
            }
        }
    }
    Ok(ExitCode::Ok)
}

pub fn launch_gui() -> anyhow::Result<()> {
    let store = ConfigStore::load()?;
    launch_gui_with_store(store)
}

fn launch_gui_with_store(store: ConfigStore) -> anyhow::Result<()> {
    gui::launch(store)
}

fn launch_gui_companion() -> Result<ExitCode> {
    let cli_exe = std::env::current_exe().context("无法取得 CLI EXE 路径")?;
    let gui_exe = cli_exe
        .parent()
        .context("CLI EXE 没有父目录")?
        .join("zhfd-checkin-gui.exe");
    if !gui_exe.is_file() {
        bail!(
            "找不到独立 GUI 程序: {}；请运行 zhfd-checkin-gui.exe",
            gui_exe.display()
        );
    }
    process::detached_command(&gui_exe)
        .spawn()
        .with_context(|| format!("无法启动 GUI: {}", gui_exe.display()))?;
    Ok(ExitCode::Ok)
}

#[allow(dead_code)]
fn write_report(store: &ConfigStore) -> Result<ExitCode> {
    let path = logger::write_diagnostic_report(store)?;
    println!("诊断报告: {}", path.display());
    Ok(ExitCode::Ok)
}

#[allow(dead_code)]
pub(crate) fn analyze_image_file(path: &std::path::Path) -> Result<vision::ButtonAnalysis> {
    let bytes = std::fs::read(path).with_context(|| format!("无法读取图片: {}", path.display()))?;
    let image = vision::decode(&bytes)?;
    let button = if image.width() <= 500 && image.height() <= 500 {
        vision::ButtonBox {
            cx: image.width() / 2,
            cy: image.height() / 2,
            width: image.width(),
            height: image.height(),
        }
    } else {
        vision::find_colored_button(&image).context("未找到彩色签到按钮")?
    };
    Ok(vision::classify_button(&image, button))
}

fn analyze_image(path: &std::path::Path) -> Result<ExitCode> {
    println!(
        "{}",
        serde_json::to_string_pretty(&analyze_image_file(path)?)?
    );
    Ok(ExitCode::Ok)
}

#[allow(dead_code)]
fn print_about() {
    println!("zhfd-checkin {}", VERSION);
    println!("Git commit: {}", GIT_COMMIT);
    println!("Rust Windows 便携式 CLI");
    println!("雷电模拟器官网: {}", LDPLAYER_URL);
    println!("智汇福大官网: {}", ZHFD_URL);
    println!("配置/日志根目录: EXE 所在目录");
    println!("支持 Profile: {}", profile::PROFILES.len());
    for supported in profile::PROFILES {
        println!(
            "  {} {}x{}@{} {} calibrated={}",
            supported.id,
            supported.width,
            supported.height,
            supported.density_dpi,
            supported.orientation.as_str(),
            supported.calibrated
        );
    }
    println!("视觉依赖: image + 手写 HSV/像素统计/行结构");
    println!("设备依赖: ADB + LDPlayer ldconsole.exe");
    println!("GUI: eframe/egui + rfd");
}

fn config_command(store: &ConfigStore, command: ConfigCommand) -> Result<ExitCode> {
    match command {
        ConfigCommand::Show => {
            println!(
                "配置文件: {}\n{}",
                store.path.display(),
                toml::to_string_pretty(&store.config)?
            );
        }
        ConfigCommand::Open => {
            #[cfg(windows)]
            {
                process::command("cmd")
                    .args(["/C", "start", "", &store.path.to_string_lossy()])
                    .spawn()
                    .context("无法打开系统编辑器")?;
            }
            #[cfg(not(windows))]
            {
                bail!("config open 当前仅实现 Windows")
            }
        }
    }
    Ok(ExitCode::Ok)
}

#[allow(dead_code)]
fn profile_command(store: &ConfigStore, command: ProfileCommand) -> Result<ExitCode> {
    match command {
        ProfileCommand::List => {
            for p in profile::PROFILES {
                println!(
                    "{} {}x{}@{} {} calibrated={}",
                    p.id,
                    p.width,
                    p.height,
                    p.density_dpi,
                    p.orientation.as_str(),
                    p.calibrated
                );
            }
        }
        ProfileCommand::Detect { target } => {
            let (_, device, info) = runtime_with_target(store, false, Some(&target))?;
            print_device(&device.serial, &info);
            match profile::find_profile(info.width, info.height, info.density_dpi) {
                Some(p) => println!("profile={}", p.id),
                None => println!("profile=unknown"),
            };
        }
        ProfileCommand::Sync {
            id,
            confirm,
            target,
        } => {
            sync_profile(store, &id, confirm, &target)?;
        }
    }
    Ok(ExitCode::Ok)
}

fn sync_profile(
    store: &ConfigStore,
    id: &str,
    confirm: bool,
    instance_target: &InstanceTargetArgs,
) -> Result<ExitCode> {
    if !confirm {
        bail!("同步模拟器配置会修改分辨率/DPI并重启实例，请指定 --confirm");
    }
    let target =
        profile::find_profile_by_id(id).with_context(|| format!("未知 profile: {}", id))?;
    let target_geometry = SyncGeometry {
        source: "requested_profile".into(),
        width: Some(target.width),
        height: Some(target.height),
        density_dpi: Some(target.density_dpi),
        profile_id: Some(target.id.to_string()),
    };
    let sync_log = SyncLog::new(&store.root)?;
    let mut record = SyncResult {
        action: "profile_sync".into(),
        profile_id: target.id.to_string(),
        instance_index: instance_target
            .instance_index
            .unwrap_or(store.config.emulator.instance_index),
        target: target_geometry,
        before: None,
        after: None,
        backup_path: None,
        mutation_attempted: false,
        modified: false,
        restarted: false,
        rollback_attempted: false,
        rollback_succeeded: false,
        status: "started".into(),
        error: None,
    };
    sync_log.event("start", &record)?;

    let operation = sync_profile_inner(store, target, instance_target, &sync_log, &mut record);
    match operation {
        Ok(()) => {
            record.status = "success".into();
            sync_log.event("verified", &record)?;
            sync_log.save_json("sync.json", &record)?;
            println!("同步完成；结构化日志: {}", sync_log.dir.display());
            println!("model/IMEI/账号/定位数据未修改");
            Ok(ExitCode::Ok)
        }
        Err(error) => {
            record.status = "failed".into();
            record.error = Some(format!("{:#}", error));
            let _ = sync_log.event("failure", &record);
            let _ = sync_log.save_json("sync.json", &record);
            Err(error)
        }
    }
}

fn sync_profile_inner(
    store: &ConfigStore,
    target: &profile::Profile,
    instance_target: &InstanceTargetArgs,
    sync_log: &SyncLog,
    record: &mut SyncResult,
) -> Result<()> {
    let ld = ldplayer::LdPlayer::discover(
        &configured_path(store, &store.config.emulator.ldconsole_path),
        &store.root,
    )?;
    let selected = resolve_instance_target(&ld, store, Some(instance_target))?;
    record.instance_index = selected.index;
    let index = selected.index;
    let instance = ld.instance(index)?;
    let source = ld.executable.parent().map(|parent| {
        parent
            .join("vms")
            .join("config")
            .join(format!("leidian{}.config", index))
    });
    let backup = source.as_ref().filter(|path| path.is_file()).map(|path| {
        path.with_extension(format!(
            "config.bak.{}",
            chrono::Local::now().format("%Y%m%d%H%M%S")
        ))
    });

    let mut before = geometry_from_instance(&instance);
    if instance.running {
        match runtime_with_target(store, false, Some(instance_target)) {
            Ok((_, _, info)) => before = geometry_from_device(&info),
            Err(error) => {
                sync_log.event(
                    "before_read_warning",
                    serde_json::json!({"error": format!("{:#}", error)}),
                )?;
            }
        }
    }
    record.before = Some(before.clone());
    sync_log.event("before", &before)?;

    let operation: Result<()> = (|| {
        if let (Some(source), Some(backup)) = (
            source.as_ref().filter(|path| path.is_file()),
            backup.as_ref(),
        ) {
            std::fs::copy(source, backup)
                .with_context(|| format!("无法备份 LDPlayer 配置: {}", source.display()))?;
            record.backup_path = Some(backup.display().to_string());
            sync_log.event(
                "backup_created",
                serde_json::json!({"source": source, "backup": backup}),
            )?;
        } else {
            sync_log.event("backup_unavailable", serde_json::json!({"source": source}))?;
        }

        let resolution = format!("{},{},{}", target.width, target.height, target.density_dpi);
        record.mutation_attempted = true;
        ld.command(&[
            "modify",
            "--index",
            &index.to_string(),
            "--resolution",
            &resolution,
        ])?;
        record.modified = true;
        sync_log.event(
            "modified",
            serde_json::json!({
                "index": index,
                "resolution": resolution,
                "width": target.width,
                "height": target.height,
                "density_dpi": target.density_dpi,
            }),
        )?;

        if instance.running {
            ld.command(&["quit", "--index", &index.to_string()])?;
            ld.wait_for_stopped(index, Duration::from_secs(30))?;
            ld.launch_wait(
                index,
                Duration::from_secs(store.config.runtime.startup_timeout_seconds),
            )?;
            record.restarted = true;
            sync_log.event("restarted", serde_json::json!({"index": index}))?;
        }

        let after = if instance.running {
            let (_, _, info) = runtime_with_target(store, false, Some(instance_target))?;
            geometry_from_device(&info)
        } else {
            geometry_from_instance(&ld.instance(index)?)
        };
        record.after = Some(after.clone());
        sync_log.event("after", &after)?;
        if !geometry_matches_target(&after, target) {
            bail!(
                "同步后实际分辨率/DPI复核失败：实际 {:?}x{:?}@{:?}，目标 {}x{}@{}",
                after.width,
                after.height,
                after.density_dpi,
                target.width,
                target.height,
                target.density_dpi
            );
        }
        Ok(())
    })();

    if let Err(error) = operation {
        if record.mutation_attempted {
            record.rollback_attempted = true;
            sync_log.event("rollback_started", serde_json::json!({"index": index}))?;
            match restore_sync_backup(
                &ld,
                index,
                source.as_deref(),
                backup.as_deref(),
                instance.running,
                Duration::from_secs(store.config.runtime.startup_timeout_seconds),
            ) {
                Ok(()) => {
                    record.rollback_succeeded = true;
                    sync_log.event("rollback_succeeded", serde_json::json!({"index": index}))?;
                }
                Err(recovery_error) => {
                    sync_log.event(
                        "rollback_failed",
                        serde_json::json!({"error": format!("{:#}", recovery_error)}),
                    )?;
                    return Err(anyhow::anyhow!(
                        "{}；自动恢复备份失败: {:#}",
                        error,
                        recovery_error
                    ));
                }
            }
        }
        return Err(error);
    }
    Ok(())
}

fn geometry_from_instance(instance: &ldplayer::Instance) -> SyncGeometry {
    let profile_id = match (instance.width, instance.height, instance.dpi) {
        (Some(width), Some(height), Some(dpi)) => {
            profile::find_profile(width, height, dpi).map(|profile| profile.id.to_string())
        }
        _ => None,
    };
    SyncGeometry {
        source: "ldplayer_list2".into(),
        width: instance.width,
        height: instance.height,
        density_dpi: instance.dpi,
        profile_id,
    }
}

fn geometry_from_device(info: &adb::DeviceInfo) -> SyncGeometry {
    SyncGeometry {
        source: "adb_wm".into(),
        width: Some(info.width),
        height: Some(info.height),
        density_dpi: Some(info.density_dpi),
        profile_id: profile::find_profile(info.width, info.height, info.density_dpi)
            .map(|profile| profile.id.to_string()),
    }
}

fn geometry_matches_target(geometry: &SyncGeometry, target: &profile::Profile) -> bool {
    geometry.width == Some(target.width)
        && geometry.height == Some(target.height)
        && geometry.density_dpi == Some(target.density_dpi)
}

trait SyncLifecycle {
    fn is_running(&self, index: u32) -> Result<bool>;
    fn stop_and_wait(&self, index: u32) -> Result<()>;
    fn launch_wait(&self, index: u32, timeout: Duration) -> Result<()>;
}

impl SyncLifecycle for ldplayer::LdPlayer {
    fn is_running(&self, index: u32) -> Result<bool> {
        Ok(self.instance(index)?.running)
    }

    fn stop_and_wait(&self, index: u32) -> Result<()> {
        self.command(&["quit", "--index", &index.to_string()])?;
        self.wait_for_stopped(index, Duration::from_secs(30))
    }

    fn launch_wait(&self, index: u32, timeout: Duration) -> Result<()> {
        ldplayer::LdPlayer::launch_wait(self, index, timeout)
    }
}

fn restore_sync_backup<R: SyncLifecycle>(
    controller: &R,
    index: u32,
    source: Option<&std::path::Path>,
    backup: Option<&std::path::Path>,
    was_running: bool,
    startup_timeout: Duration,
) -> Result<()> {
    let source = source.context("没有找到 LDPlayer 配置源文件，无法恢复")?;
    let backup = backup.context("没有生成 LDPlayer 配置备份，无法恢复")?;
    let currently_running = controller.is_running(index).unwrap_or(was_running);
    if currently_running {
        controller.stop_and_wait(index)?;
    }
    std::fs::copy(backup, source).with_context(|| {
        format!(
            "恢复 LDPlayer 配置失败: {} -> {}",
            backup.display(),
            source.display()
        )
    })?;
    if was_running {
        controller.launch_wait(index, startup_timeout)?;
    }
    Ok(())
}

fn configured_path(store: &ConfigStore, value: &str) -> String {
    config::resolve_path(&store.root, value)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[derive(Debug, Clone)]
pub(crate) struct DependencyDiagnostics {
    pub ldplayer: discovery::DiscoveryReport,
    pub adb: discovery::DiscoveryReport,
}

pub(crate) fn dependency_diagnostics(store: &ConfigStore) -> DependencyDiagnostics {
    let ldplayer = ldplayer::LdPlayer::discover_report(
        &configured_path(store, &store.config.emulator.ldconsole_path),
        &store.root,
    );
    let adb = adb::discover_adb(
        &store.config.emulator.adb_path,
        ldplayer
            .selected()
            .map(|candidate| candidate.path.as_path()),
        Some(&store.root),
    );
    DependencyDiagnostics { ldplayer, adb }
}

fn diagnose(store: &ConfigStore, instance_target: &InstanceTargetArgs) -> Result<ExitCode> {
    let report = dependency_diagnostics(store);
    println!("{}", report.ldplayer.summary());
    println!("{}", report.adb.summary());
    let Some(ld_path) = report.ldplayer.selected_path() else {
        return Ok(ExitCode::DeviceFailure);
    };
    let ld = ldplayer::LdPlayer {
        executable: ld_path,
    };
    let instances = ld.instances()?;
    if instances.is_empty() {
        println!("实例: none");
        return Ok(ExitCode::DeviceFailure);
    }
    for instance in &instances {
        println!(
            "instance={} name={} running={} ldplayer_reported_port={:?} size={:?}x{:?} dpi={:?}",
            instance.index,
            instance.name,
            instance.running,
            instance.ldplayer_reported_port,
            instance.width,
            instance.height,
            instance.dpi
        );
    }
    let (_, device, info) = runtime_with_target(store, false, Some(instance_target))?;
    print_device(&device.serial, &info);
    if let Some(profile) = profile::find_profile(info.width, info.height, info.density_dpi) {
        println!("profile={} calibrated={}", profile.id, profile.calibrated);
    } else {
        println!("profile=unknown");
    }
    Ok(ExitCode::Ok)
}

#[derive(Debug, Clone)]
pub(crate) struct ApkInstallResult {
    pub install_output: String,
    pub package_name: String,
    pub version: String,
}

pub(crate) fn install_apk(
    store: &ConfigStore,
    apk: &std::path::Path,
    instance_target: &InstanceTargetArgs,
) -> Result<ApkInstallResult> {
    let metadata = apk::inspect_apk(apk)?;
    if metadata.package_name != store.config.app.package_name {
        bail!(
            "APK 包名不匹配：实际={}，目标={}",
            metadata.package_name,
            store.config.app.package_name
        );
    }
    let (_, device, _) = runtime_with_target(store, true, Some(instance_target))?;
    install_apk_on_device(store, apk, &device)
}

fn install_apk_on_device(
    store: &ConfigStore,
    apk: &std::path::Path,
    device: &adb::AdbDevice,
) -> Result<ApkInstallResult> {
    let install_output = device.install_apk(apk)?;
    let version = device.package_version(&store.config.app.package_name)?;
    let mut saved = store.clone();
    saved.config.app.apk_path = config::path_for_config(&store.root, apk);
    saved.save()?;
    Ok(ApkInstallResult {
        install_output,
        package_name: store.config.app.package_name.clone(),
        version,
    })
}

pub(crate) fn inspect_apk_metadata(apk: &std::path::Path) -> Result<apk::ApkMetadata> {
    apk::inspect_apk(apk)
}

pub(crate) fn installed_app_version(
    store: &ConfigStore,
    instance_target: &InstanceTargetArgs,
) -> Result<String> {
    let (_, device, _) = runtime_with_target(store, false, Some(instance_target))?;
    device.package_version(&store.config.app.package_name)
}

#[allow(dead_code)]
fn app_command(store: &ConfigStore, command: AppCommand) -> Result<ExitCode> {
    match command {
        AppCommand::Install { apk, target } => {
            println!("安装 APK: {}", apk.display());
            let result = install_apk(store, &apk, &target)?;
            println!("{}", result.install_output);
            println!("安装后包校验: {} {}", result.package_name, result.version);
            Ok(ExitCode::Ok)
        }
    }
}

fn build_task_command_line(
    exe: &std::path::Path,
    mode: &str,
    all_instances: bool,
    target: &InstanceTargetArgs,
) -> String {
    let mut task_run = format!("\"{}\" {}", exe.display(), mode);
    if all_instances {
        task_run.push_str(" --all-instances");
    } else if let Some(index) = target.instance_index {
        task_run.push_str(&format!(" --instance-index {}", index));
    }
    if let Some(serial) = target
        .serial
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        task_run.push_str(&format!(" --serial {}", serial));
    }
    task_run
}

fn cli_executable_path(current_exe: &std::path::Path) -> std::path::PathBuf {
    let file_name = current_exe
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if file_name.eq_ignore_ascii_case("zhfd-checkin-gui.exe") {
        return current_exe.with_file_name("zhfd-checkin.exe");
    }
    if file_name.eq_ignore_ascii_case("zhfd-checkin-gui") {
        return current_exe.with_file_name("zhfd-checkin");
    }
    current_exe.to_path_buf()
}

fn task_command(command: TaskCommand) -> Result<ExitCode> {
    match command {
        TaskCommand::Create {
            name,
            live,
            confirm,
            all_instances,
            target,
        } => {
            if live && !confirm {
                bail!("正式计划任务必须同时指定 --live --confirm");
            }
            let current_exe = std::env::current_exe().context("无法取得 EXE 路径")?;
            let exe = cli_executable_path(&current_exe);
            let mode = if live {
                "run --live --confirm"
            } else {
                "run --dry-run"
            };
            let task_run = build_task_command_line(&exe, mode, all_instances, &target);
            let store = ConfigStore::load()?;
            let start = store.config.window.start.clone();
            let output = process::command("schtasks.exe")
                .args([
                    "/Create", "/TN", &name, "/TR", &task_run, "/SC", "DAILY", "/ST", &start, "/F",
                ])
                .output()
                .context("无法执行 schtasks.exe")?;
            if !output.status.success() {
                bail!(
                    "创建计划任务失败: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            println!("已创建计划任务 {}: {}", name, task_run);
        }
        TaskCommand::Delete { name } => {
            let output = process::command("schtasks.exe")
                .args(["/Delete", "/TN", &name, "/F"])
                .output()
                .context("无法执行 schtasks.exe")?;
            if !output.status.success() {
                bail!(
                    "删除计划任务失败: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            println!("已删除计划任务 {}", name);
        }
        TaskCommand::Query { name } => {
            let output = process::command("schtasks.exe")
                .args(["/Query", "/TN", &name, "/FO", "LIST"])
                .output()
                .context("无法执行 schtasks.exe")?;
            println!("{}", String::from_utf8_lossy(&output.stdout));
            if !output.status.success() {
                return Ok(ExitCode::BusinessFailure);
            }
        }
    }
    Ok(ExitCode::Ok)
}

#[allow(dead_code)]
pub(crate) fn dump_ui_hierarchy(
    store: &ConfigStore,
    target: &InstanceTargetArgs,
) -> Result<String> {
    let (_, device, _) = runtime_with_target(store, false, Some(target))?;
    device.dump_hierarchy()
}

fn ui_command(store: &ConfigStore, command: UiCommand) -> Result<ExitCode> {
    match command {
        UiCommand::Dump { target } => println!("{}", dump_ui_hierarchy(store, &target)?),
    }
    Ok(ExitCode::Ok)
}

fn run(store: &ConfigStore, args: RunArgs) -> Result<ExitCode> {
    let mode = if args.live {
        Mode::Live
    } else if args.dry_run {
        Mode::DryRun
    } else {
        Mode::from_config(&store.config.runtime.mode)?
    };
    if args.all_instances {
        return run_all_instances(store, args);
    }
    let log = RunLog::new(&store.root)?;
    let mut result = RunResult {
        schema_version: 2,
        run_id: log
            .dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        started_at: chrono::Local::now().to_rfc3339(),
        finished_at: None,
        mode: mode.as_str().into(),
        instance_index: args.target.instance_index,
        instance_name: None,
        serial: String::new(),
        profile_id: None,
        screen_width: None,
        screen_height: None,
        density_dpi: None,
        state_history: vec![],
        clicked: false,
        success: false,
        dry_run_ready: false,
        field_work_ready: false,
        page_route: None,
        checkin_mode: None,
        checkin_state: None,
        success_date: None,
        error_stage: None,
        recovery_action: None,
        artifact_paths: Vec::new(),
        error: None,
    };
    seed_result_identity(store, &args.target, &mut result);
    log.event("start", serde_json::json!({"mode":mode.as_str()}))?;
    if mode == Mode::Live && !args.confirm {
        set_run_failure(
            &mut result,
            "live_confirmation_required",
            "click_authorization",
            "正式模式需要显式 --confirm；未执行签到点击。",
        );
        finalize_run(&log, &mut result)?;
        println!("结果: {}", serde_json::to_string_pretty(&result)?);
        return Ok(ExitCode::BusinessFailure);
    }

    let execution = run_inner(store, &args, mode, &log, &mut result);
    let code = match execution {
        Ok(code) => code,
        Err(error) => {
            if result.error.is_none() {
                result.error = Some(format!("{:#}", error));
            }
            if result.error_stage.is_none() {
                result.error_stage = Some("runtime".into());
            }
            if result.recovery_action.is_none() {
                result.recovery_action = Some("请查看诊断报告和最新运行截图后重试。".into());
            }
            eprintln!("运行失败: {:#}", error);
            ExitCode::DeviceFailure
        }
    };

    finalize_run(&log, &mut result)?;
    println!("结果: {}", serde_json::to_string_pretty(&result)?);
    Ok(code)
}

fn finalize_run(log: &RunLog, result: &mut RunResult) -> Result<()> {
    result.finished_at = Some(chrono::Local::now().to_rfc3339());
    result.artifact_paths = [
        log.dir.join("result.json"),
        log.dir.join("events.jsonl"),
        log.dir.join("screenshots"),
    ]
    .iter()
    .map(|path| path.display().to_string())
    .collect();
    log.save_json("result.json", result)?;
    log.event("finish", result)?;
    Ok(())
}

fn run_all_instances(store: &ConfigStore, args: RunArgs) -> Result<ExitCode> {
    if args.target.instance_index.is_some() || args.target.serial.is_some() {
        bail!("--all-instances 不能与 --instance-index 或 --serial 同时使用");
    }
    let configured = store.config.emulator.effective_instances();
    if configured.is_empty() {
        bail!("没有启用的 LDPlayer 实例");
    }
    let mut overall = ExitCode::Ok;
    for configured_instance in configured {
        let mut per_instance = args.clone();
        per_instance.all_instances = false;
        per_instance.target = InstanceTargetArgs {
            instance_index: Some(configured_instance.instance_index),
            serial: (!configured_instance.serial.trim().is_empty())
                .then_some(configured_instance.serial.clone()),
        };
        println!(
            "=== 运行实例 {} ({}) serial={} ===",
            configured_instance.instance_index,
            if configured_instance.name.trim().is_empty() {
                "未命名"
            } else {
                configured_instance.name.as_str()
            },
            per_instance
                .target
                .serial
                .as_deref()
                .unwrap_or("按 index 推导")
        );
        let code = run(store, per_instance)?;
        if code as i32 > overall as i32 {
            overall = code;
        }
    }
    Ok(overall)
}

fn run_inner(
    store: &ConfigStore,
    args: &RunArgs,
    mode: Mode,
    log: &RunLog,
    result: &mut RunResult,
) -> Result<ExitCode> {
    run_inner_with_device(
        store,
        args,
        mode,
        log,
        result,
        |store, launch, target, result| {
            runtime_with_target_recording(store, launch, Some(target), Some(result))
        },
    )
}

fn seed_result_identity(store: &ConfigStore, target: &InstanceTargetArgs, result: &mut RunResult) {
    let configured_instances = store.config.emulator.effective_instances();
    let explicit_serial = target
        .serial
        .as_deref()
        .filter(|serial| !serial.trim().is_empty());
    let index_from_serial = explicit_serial.and_then(|serial| {
        configured_instances
            .iter()
            .find(|instance| instance.serial.trim() == serial)
            .map(|instance| instance.instance_index)
            .or_else(|| {
                configured_instances.iter().find_map(|instance| {
                    (ldplayer::LdPlayer::tcp_serial_for_index(instance.instance_index).as_deref()
                        == Some(serial))
                    .then_some(instance.instance_index)
                })
            })
    });
    let index = target.instance_index.or(index_from_serial).or_else(|| {
        explicit_serial
            .is_none()
            .then_some(store.config.emulator.instance_index)
    });
    result.instance_index = index;
    result.serial = explicit_serial
        .map(str::to_string)
        .or_else(|| {
            index.and_then(|index| {
                configured_instances
                    .iter()
                    .find(|instance| instance.instance_index == index)
                    .filter(|instance| !instance.serial.trim().is_empty())
                    .map(|instance| instance.serial.clone())
            })
        })
        .or_else(|| {
            index
                .filter(|index| *index == store.config.emulator.instance_index)
                .and_then(|_| {
                    let serial = store.config.emulator.serial.trim();
                    (!serial.is_empty()).then(|| serial.to_string())
                })
        })
        .or_else(|| index.and_then(ldplayer::LdPlayer::tcp_serial_for_index))
        .unwrap_or_default();
    result.instance_name = index.and_then(|index| {
        configured_instances
            .iter()
            .find(|instance| instance.instance_index == index)
            .map(|instance| instance.name.clone())
            .filter(|name| !name.trim().is_empty())
    });
}

fn set_run_failure(result: &mut RunResult, code: &str, stage: &str, recovery_action: &str) {
    result.error = Some(code.into());
    result.error_stage = Some(stage.into());
    result.recovery_action = Some(recovery_action.into());
}

fn checkin_state_from_visual(state: vision::ButtonState) -> CheckinState {
    match state {
        vision::ButtonState::Gray => CheckinState::Gray,
        vision::ButtonState::Locating => CheckinState::Locating,
        vision::ButtonState::Ready => CheckinState::Ready,
        vision::ButtonState::Success | vision::ButtonState::Unknown => CheckinState::Unknown,
    }
}

fn extract_dates_from_text(text: &str) -> Vec<chrono::NaiveDate> {
    let mut dates = Vec::new();
    for (start, ch) in text.char_indices() {
        if ch != '2' {
            continue;
        }
        let candidate: String = text[start..]
            .chars()
            .take_while(|ch| {
                ch.is_ascii_digit() || matches!(*ch, '-' | '/' | '.' | '年' | '月' | '日')
            })
            .collect();
        let normalized = candidate
            .replace(['年', '月'], "-")
            .replace('日', "")
            .replace(['/', '.'], "-");
        let parts: Vec<_> = normalized
            .split('-')
            .filter(|part| !part.is_empty())
            .collect();
        if parts.len() >= 3
            && let (Ok(year), Ok(month), Ok(day)) = (
                parts[0].parse::<i32>(),
                parts[1].parse::<u32>(),
                parts[2].parse::<u32>(),
            )
            && (2000..=2100).contains(&year)
            && let Some(date) = chrono::NaiveDate::from_ymd_opt(year, month, day)
            && !dates.contains(&date)
        {
            dates.push(date);
        }
    }
    dates
}

fn extract_hierarchy_date(xml: &str) -> Option<chrono::NaiveDate> {
    let mut all_dates = Vec::new();
    let mut contextual_dates = Vec::new();
    for attribute in ["text=\"", "content-desc=\""] {
        let mut cursor = 0;
        while let Some(relative) = xml.get(cursor..)?.find(attribute) {
            let value_start = cursor + relative + attribute.len();
            let value_length = xml.get(value_start..)?.find('"')?;
            let value_end = value_start + value_length;
            let value = &xml[value_start..value_end];
            let has_checkin_context = value.contains("签到成功") || value.contains("已签到");
            let has_explicit_date_label = value.trim_start().starts_with("日期");
            for date in extract_dates_from_text(value) {
                if !all_dates.contains(&date) {
                    all_dates.push(date);
                }
                if (has_checkin_context || has_explicit_date_label)
                    && !contextual_dates.contains(&date)
                {
                    contextual_dates.push(date);
                }
            }
            cursor = value_end + 1;
        }
    }
    if contextual_dates.len() != 1 || all_dates.iter().any(|date| *date != contextual_dates[0]) {
        return None;
    }
    contextual_dates.first().copied()
}
fn run_inner_with_device<F>(
    store: &ConfigStore,
    args: &RunArgs,
    mode: Mode,
    log: &RunLog,
    result: &mut RunResult,
    runtime_provider: F,
) -> Result<ExitCode>
where
    F: FnOnce(
        &ConfigStore,
        bool,
        &InstanceTargetArgs,
        &mut RunResult,
    ) -> Result<(ldplayer::LdPlayer, adb::AdbDevice, adb::DeviceInfo)>,
{
    result.error_stage = Some("device_setup".into());
    let (ld, device, info) = runtime_provider(store, true, &args.target, result)?;
    result.serial = device.serial.clone();
    let selected = resolve_instance_target(&ld, store, Some(&args.target))?;
    result.instance_index = Some(selected.index);
    result.instance_name = Some(selected.name);

    result.error_stage = Some("profile_detection".into());
    let profile = match profile::find_profile(info.width, info.height, info.density_dpi) {
        Some(profile) => profile,
        None => {
            set_run_failure(
                result,
                "unknown_profile",
                "profile_detection",
                "同步或选择与设备分辨率/DPI 匹配的 Profile 后重试；未执行签到点击。",
            );
            log.event(
                "profile_unknown",
                serde_json::json!({"width":info.width,"height":info.height,"dpi":info.density_dpi}),
            )?;
            return Ok(ExitCode::VisionFailure);
        }
    };
    result.profile_id = Some(profile.id.to_string());
    result.screen_width = Some(info.width);
    result.screen_height = Some(info.height);
    result.density_dpi = Some(info.density_dpi);
    if mode == Mode::Live && !profile.calibrated {
        set_run_failure(
            result,
            "uncalibrated_profile",
            "click_authorization",
            "选择已标定 Profile；未执行签到点击。",
        );
        return Ok(ExitCode::BusinessFailure);
    }

    result.error_stage = Some("app_launch".into());
    device.launch_package(
        &store.config.app.package_name,
        &store.config.app.activity_name,
    )?;
    result.error_stage = Some("app_foreground".into());
    if wait_for_package(
        &device,
        &store.config.app.package_name,
        Duration::from_secs(store.config.runtime.startup_timeout_seconds),
    )
    .is_err()
    {
        set_run_failure(
            result,
            "app_start_timeout",
            "app_foreground",
            "请在模拟器中手动打开目标 App 并完成登录，然后重新诊断/运行。",
        );
        return Ok(ExitCode::Timeout);
    }
    thread::sleep(Duration::from_millis(800));

    result.error_stage = Some("navigation".into());
    let navigation = locate_or_navigate(&device, profile, log)?;
    result.page_route = Some(navigation.route);
    result.checkin_mode = Some(navigation.checkin_mode);
    log.event(
        "page_observation",
        serde_json::json!({
            "route": navigation.route.as_str(),
            "checkin_mode": navigation.checkin_mode.as_str(),
        }),
    )?;
    if navigation.route != PageRoute::Checkin {
        let (code, recovery) = if navigation.route == PageRoute::Login {
            (
                "login_required",
                "请在模拟器中手动完成登录；本程序不会输入账号、密码、验证码或点击一键登录。",
            )
        } else {
            (
                "navigation_not_confirmed",
                "请手动进入“晚点名签到”页面后重新运行；未执行签到点击。",
            )
        };
        set_run_failure(result, code, "navigation", recovery);
        return Ok(ExitCode::VisionFailure);
    }
    if navigation.checkin_mode == CheckinMode::Unsupported {
        result.checkin_state = Some(CheckinState::Unknown);
        set_run_failure(
            result,
            "unsupported_checkin_mode",
            "checkin_classification",
            "当前签到按钮文案/业务模式未识别；请人工确认页面并提供脱敏 hierarchy 证据。",
        );
        return Ok(ExitCode::BusinessFailure);
    }

    if navigation.checkin_mode == CheckinMode::FieldWork {
        result.error_stage = Some("field_work_observation".into());
        log.event(
            "field_work_observation",
            serde_json::json!({
                "route": navigation.route.as_str(),
                "mode": navigation.checkin_mode.as_str(),
                "state": "observation_pending",
                "clicked": false,
            }),
        )?;
        if mode == Mode::Live {
            result.checkin_state = Some(CheckinState::Unknown);
            set_run_failure(
                result,
                "field_work_live_unsupported",
                "click_authorization",
                "外勤签到尚未开放自动点击；请在模拟器中人工确认并完成签到。",
            );
            return Ok(ExitCode::BusinessFailure);
        }

        let bytes = device.screenshot()?;
        log.save_bytes("field_work_observation.png", &bytes)?;
        if navigation.field_work_ready {
            result
                .state_history
                .push(CheckinState::FieldWorkReady.as_str().into());
            result.checkin_state = Some(CheckinState::FieldWorkReady);
            result.field_work_ready = true;
            result.error_stage = None;
            log.event(
                "field_work_visual_state",
                serde_json::json!({"state":"field_work_ready","clicked":false}),
            )?;
            return Ok(ExitCode::Ok);
        }
        result.checkin_state = Some(CheckinState::Unknown);
        set_run_failure(
            result,
            "field_work_observation_only",
            "field_work_observation",
            "外勤页面状态无法从层级结构确认；仅保存 dry-run 截图，请人工确认并完成签到。",
        );
        log.event(
            "field_work_visual_state",
            serde_json::json!({"state":"unknown","clicked":false}),
        )?;
        return Ok(ExitCode::BusinessFailure);
    }

    // Page route and mode remain explicit and are re-read before every state
    // observation. A changed or unobservable page never enters the tap path.
    let started = Instant::now();
    let mut unknowns = ConsecutiveUnknown::default();
    let mut ready_frames = 0u32;
    let mut click_deadline: Option<Instant> = None;
    loop {
        if click_deadline.is_none() {
            result.error_stage = Some("time_window".into());
            let within_window = domain::in_configured_window(
                &store.config.window.start,
                &store.config.window.end,
                &store.config.window.timezone,
            )?;
            if !within_window {
                set_run_failure(
                    result,
                    "outside_window",
                    "time_window",
                    "请在配置的签到时间窗口内重新运行。",
                );
                break;
            }
            if started.elapsed()
                >= Duration::from_secs(store.config.runtime.location_timeout_seconds)
            {
                set_run_failure(
                    result,
                    "location_timeout",
                    "checkin_wait",
                    "请检查网络/定位权限和页面状态，之后重新运行。",
                );
                break;
            }
        }

        result.error_stage = Some("page_observation".into());
        let hierarchy = match device.dump_hierarchy() {
            Ok(hierarchy) => hierarchy,
            Err(_) => {
                set_run_failure(
                    result,
                    "hierarchy_unavailable",
                    "page_observation",
                    "请保持目标签到页在前台并重新运行诊断；页面无法确认时不会点击。",
                );
                break;
            }
        };
        let current_route = detect_page_route(&hierarchy);
        let current_mode = detect_checkin_mode(&hierarchy);
        result.page_route = Some(current_route);
        result.checkin_mode = Some(current_mode);
        if current_route != PageRoute::Checkin || current_mode != CheckinMode::Ordinary {
            set_run_failure(
                result,
                "page_context_changed",
                "page_observation",
                "请返回普通晚点名签到页面并重新运行；程序已停止自动点击。",
            );
            break;
        }

        result.error_stage = Some("visual_observation".into());
        let bytes = device.screenshot()?;
        log.save_bytes(
            &format!("state_{}.png", result.state_history.len() + 1),
            &bytes,
        )?;
        let image = vision::decode(&bytes)?;
        let button = vision::find_colored_button(&image)
            .unwrap_or_else(|| vision::fallback_button(&image, profile));
        let analysis = vision::classify_button(&image, button);
        let state = format!("{:?}", analysis.state).to_lowercase();
        result.state_history.push(state.clone());
        result.checkin_state = Some(checkin_state_from_visual(analysis.state));
        log.event("state", &analysis)?;

        if analysis.state == vision::ButtonState::Success {
            result.error_stage = Some("success_confirmation".into());
            if !["签到成功", "已签到"]
                .iter()
                .any(|marker| hierarchy.contains(marker))
            {
                result.checkin_state = Some(CheckinState::Unknown);
                log.event(
                    "checkin_semantic_state",
                    serde_json::json!({
                        "route": current_route.as_str(),
                        "mode": current_mode.as_str(),
                        "state": CheckinState::Unknown.as_str(),
                        "reason": "success_text_unconfirmed",
                    }),
                )?;
                if unknowns.observe(true) >= store.config.runtime.unknown_retries {
                    set_run_failure(
                        result,
                        "success_text_unconfirmed",
                        "success_confirmation",
                        "页面视觉与层级文字不一致；请人工确认签到结果。",
                    );
                    break;
                }
                thread::sleep(Duration::from_secs(1));
                continue;
            }
            match extract_hierarchy_date(&hierarchy) {
                Some(detected_date) => {
                    let today = domain::configured_date(&store.config.window.timezone)?;
                    result.success_date = Some(detected_date.to_string());
                    if detected_date == today {
                        result.checkin_state = Some(CheckinState::SuccessToday);
                        result.success = true;
                        result.error_stage = None;
                        result.recovery_action = None;
                    } else {
                        result.checkin_state = Some(CheckinState::SuccessStale);
                        set_run_failure(
                            result,
                            "success_stale",
                            "success_confirmation",
                            "签到页面显示的成功日期不是当前配置时区的今天；请确认是否需要签到。",
                        );
                    }
                }
                None => {
                    result.checkin_state = Some(CheckinState::SuccessDateUnknown);
                    set_run_failure(
                        result,
                        "success_date_unknown",
                        "success_confirmation",
                        "无法从签到页面确认成功日期；请人工核对，不按当天成功处理。",
                    );
                }
            }
            log.event(
                "checkin_semantic_state",
                serde_json::json!({
                    "route": current_route.as_str(),
                    "mode": current_mode.as_str(),
                    "state": result.checkin_state.map(|state| state.as_str()),
                    "success_date": result.success_date.as_deref(),
                }),
            )?;
            break;
        }
        log.event(
            "checkin_semantic_state",
            serde_json::json!({
                "route": current_route.as_str(),
                "mode": current_mode.as_str(),
                "state": result.checkin_state.map(|state| state.as_str()),
            }),
        )?;

        if let Some(deadline) = click_deadline {
            if deadline.elapsed()
                > Duration::from_secs(store.config.runtime.success_timeout_seconds)
            {
                set_run_failure(
                    result,
                    "success_timeout",
                    "success_confirmation",
                    "本次点击后没有确认到当天成功；请人工核对页面，避免重复点击。",
                );
                break;
            }
            // After one authorized tap, never revisit the Ready tap branch.
            thread::sleep(Duration::from_secs(3));
            continue;
        }

        match analysis.state {
            vision::ButtonState::Gray | vision::ButtonState::Locating => {
                ready_frames = 0;
                unknowns.reset();
                thread::sleep(Duration::from_secs(
                    store.config.runtime.locating_poll_seconds,
                ));
            }
            vision::ButtonState::Ready => {
                let within_window = domain::in_configured_window(
                    &store.config.window.start,
                    &store.config.window.end,
                    &store.config.window.timezone,
                )?;
                if !within_window {
                    set_run_failure(
                        result,
                        "outside_window",
                        "time_window",
                        "请在配置的签到时间窗口内重新运行。",
                    );
                    break;
                }
                ready_frames += 1;
                unknowns.reset();
                if ready_frames < store.config.runtime.stable_frames {
                    thread::sleep(Duration::from_millis(700));
                    continue;
                }
                if mode == Mode::DryRun {
                    result.dry_run_ready = true;
                    result.error_stage = None;
                    break;
                }

                result.error_stage = Some("click_authorization".into());
                let confirmation_hierarchy = device.dump_hierarchy()?;
                let confirmed_route = detect_page_route(&confirmation_hierarchy);
                let confirmed_mode = detect_checkin_mode(&confirmation_hierarchy);
                if confirmed_route != PageRoute::Checkin || confirmed_mode != CheckinMode::Ordinary
                {
                    set_run_failure(
                        result,
                        "page_context_changed",
                        "click_authorization",
                        "请确认普通签到页仍处于前台；程序未执行点击。",
                    );
                    break;
                }
                let foreground_matches =
                    device.current_package().unwrap_or_default() == store.config.app.package_name;
                if !foreground_matches {
                    set_run_failure(
                        result,
                        "foreground_package_changed",
                        "click_authorization",
                        "请将目标 App 的普通签到页置于前台后重新运行；程序未执行点击。",
                    );
                    break;
                }
                let confirm_bytes = device.screenshot()?;
                let confirm_image = vision::decode(&confirm_bytes)?;
                let confirm_button = vision::find_colored_button(&confirm_image).unwrap_or(button);
                let confirm = vision::classify_button(&confirm_image, confirm_button);
                if confirm.state != vision::ButtonState::Ready {
                    set_run_failure(
                        result,
                        "ready_confirmation_failed",
                        "click_authorization",
                        "点击前 ready 二次确认失败；程序未执行点击，请重新运行。",
                    );
                    break;
                }
                result.error_stage = Some("time_window".into());
                let policy_window = domain::in_configured_window(
                    &store.config.window.start,
                    &store.config.window.end,
                    &store.config.window.timezone,
                )?;
                if !policy_window {
                    set_run_failure(
                        result,
                        "outside_window",
                        "time_window",
                        "签到时间窗口已结束；程序未执行点击。",
                    );
                    break;
                }
                result.error_stage = Some("click_authorization".into());
                match guarded_tap(
                    &device,
                    TapRequest {
                        state: confirm.state,
                        route: confirmed_route,
                        checkin_mode: confirmed_mode,
                        mode,
                        explicit_confirmation: args.confirm,
                        profile_calibrated: profile.calibrated,
                        foreground_matches,
                        within_window: policy_window,
                        ready_frames,
                        required_frames: store.config.runtime.stable_frames,
                        x: confirm_button.cx,
                        y: confirm_button.cy,
                    },
                ) {
                    Ok(true) => {}
                    Ok(false) => {
                        set_run_failure(
                            result,
                            "click_authorization_failed",
                            "click_authorization",
                            "点击门禁未授权；程序未执行点击。",
                        );
                        break;
                    }
                    Err(error) => {
                        set_run_failure(
                            result,
                            "click_authorization_failed",
                            "click_authorization",
                            "点击门禁失败；程序未执行点击。",
                        );
                        log.event(
                            "click_denied",
                            serde_json::json!({"reason":error.to_string()}),
                        )?;
                        break;
                    }
                }
                result.clicked = true;
                log.event(
                    "tap",
                    serde_json::json!({"x":confirm_button.cx,"y":confirm_button.cy}),
                )?;
                click_deadline = Some(Instant::now());
            }
            vision::ButtonState::Success => unreachable!("handled above with date evidence"),
            vision::ButtonState::Unknown => {
                ready_frames = 0;
                result.checkin_state = Some(CheckinState::Unknown);
                if unknowns.observe(true) >= store.config.runtime.unknown_retries {
                    set_run_failure(
                        result,
                        "unknown_state",
                        "visual_observation",
                        "请查看最新截图与页面层级，确认按钮状态后重新运行。",
                    );
                    break;
                }
                thread::sleep(Duration::from_secs(1));
            }
        }
    }

    let code = if result.success || result.dry_run_ready || result.field_work_ready {
        ExitCode::Ok
    } else if matches!(
        result.error.as_deref(),
        Some("location_timeout" | "success_timeout")
    ) {
        ExitCode::Timeout
    } else {
        ExitCode::BusinessFailure
    };
    Ok(code)
}

trait TapDevice {
    fn tap_at(&self, x: u32, y: u32) -> Result<()>;
}

impl TapDevice for adb::AdbDevice {
    fn tap_at(&self, x: u32, y: u32) -> Result<()> {
        self.tap(x, y)
    }
}

struct TapRequest {
    state: vision::ButtonState,
    route: PageRoute,
    checkin_mode: CheckinMode,
    mode: Mode,
    explicit_confirmation: bool,
    profile_calibrated: bool,
    foreground_matches: bool,
    within_window: bool,
    ready_frames: u32,
    required_frames: u32,
    x: u32,
    y: u32,
}

fn guarded_tap<D: TapDevice>(device: &D, request: TapRequest) -> Result<bool> {
    if request.state != vision::ButtonState::Ready {
        return Ok(false);
    }
    domain::authorize_page_action(request.route, request.checkin_mode)?;
    domain::authorize_click(
        request.mode,
        request.explicit_confirmation,
        request.profile_calibrated,
        request.foreground_matches,
        request.within_window,
        request.ready_frames,
        request.required_frames,
    )?;
    device.tap_at(request.x, request.y)?;
    Ok(true)
}

#[expect(dead_code, reason = "reserved for bounded scheduled waiting")]
fn sleep_for_next_poll(store: &ConfigStore, started: Instant) -> Result<()> {
    let location_remaining = Duration::from_secs(
        store
            .config
            .runtime
            .location_timeout_seconds
            .saturating_sub(started.elapsed().as_secs()),
    );
    let mut sleep_for = Duration::from_secs(store.config.runtime.locating_poll_seconds);
    sleep_for = sleep_for.min(location_remaining);
    if let Some(window_remaining) = domain::seconds_until_configured_window_end(
        &store.config.window.start,
        &store.config.window.end,
        &store.config.window.timezone,
    )? {
        sleep_for = sleep_for.min(Duration::from_secs(window_remaining));
    }
    if !sleep_for.is_zero() {
        thread::sleep(sleep_for);
    }
    Ok(())
}

pub(crate) fn runtime_with_target(
    store: &ConfigStore,
    launch: bool,
    instance_target: Option<&InstanceTargetArgs>,
) -> Result<(ldplayer::LdPlayer, adb::AdbDevice, adb::DeviceInfo)> {
    runtime_with_target_recording(store, launch, instance_target, None)
}

fn runtime_with_target_recording(
    store: &ConfigStore,
    launch: bool,
    instance_target: Option<&InstanceTargetArgs>,
    result: Option<&mut RunResult>,
) -> Result<(ldplayer::LdPlayer, adb::AdbDevice, adb::DeviceInfo)> {
    let ld = ldplayer::LdPlayer::discover(
        &configured_path(store, &store.config.emulator.ldconsole_path),
        &store.root,
    )?;
    let selected = resolve_instance_target(&ld, store, instance_target)?;
    if let Some(result) = result {
        result.instance_index = Some(selected.index);
        result.instance_name = Some(selected.name.clone());
        result.serial = selected.serial.clone();
    }
    let instance = ld.instance(selected.index)?;
    if launch && store.config.emulator.auto_launch && !instance.running {
        ld.launch_wait(
            instance.index,
            Duration::from_secs(store.config.runtime.startup_timeout_seconds),
        )?;
    }
    let adb_path = adb::find_adb_for_ldplayer(
        &store.config.emulator.adb_path,
        Some(&ld.executable),
        Some(&store.root),
    )?;
    let device = connect_adb_device(
        &adb_path,
        &selected.serial,
        Duration::from_secs(store.config.runtime.startup_timeout_seconds.max(1)),
    )?;
    let info = device.info();
    Ok((ld, device, info?))
}

/// LDPlayer can report an instance as running before its ADB endpoint has
/// registered with the host daemon. Keep connecting until the startup budget
/// expires instead of turning that normal boot race into an immediate GUI/CLI
/// device failure.
fn connect_adb_device(adb_path: &Path, serial: &str, timeout: Duration) -> Result<adb::AdbDevice> {
    let deadline = Instant::now() + timeout;
    loop {
        let connect_error = match process::command(adb_path)
            .args(["connect", serial])
            .output()
        {
            Ok(output) if !output.status.success() => Some(format!(
                "adb connect 失败: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )),
            Ok(_) => None,
            Err(error) => Some(format!("执行 adb connect 失败: {error}")),
        };

        let last_error = match adb::AdbDevice::discover(&adb_path.to_string_lossy(), serial) {
            Ok(device) => return Ok(device),
            Err(error) => match connect_error {
                Some(connect_error) => format!("{connect_error}; 设备状态: {error:#}"),
                None => format!("{error:#}"),
            },
        };

        if Instant::now() >= deadline {
            bail!("连接 ADB 设备超时: {} ({})", serial, last_error);
        }
        thread::sleep(Duration::from_millis(500));
    }
}

fn wait_for_package(device: &adb::AdbDevice, package: &str, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if device.current_package().unwrap_or_default() == package {
            return Ok(());
        };
        thread::sleep(Duration::from_millis(500));
    }
    bail!("等待 App 前台超时: {}", package)
}

#[derive(Debug, Clone)]
struct NavigationResult {
    route: PageRoute,
    checkin_mode: CheckinMode,
    field_work_ready: bool,
}

fn detect_checkin_mode(xml: &str) -> CheckinMode {
    if xml.contains("外勤签到") {
        return CheckinMode::FieldWork;
    }
    if [
        "点击签到",
        "无法签到",
        "未到签到时间",
        "定位中",
        "签到成功",
        "已签到",
    ]
    .iter()
    .any(|label| xml.contains(label))
    {
        return CheckinMode::Ordinary;
    }
    CheckinMode::Unsupported
}

fn detect_page_route(xml: &str) -> PageRoute {
    if xml.contains("一键登录") || xml.contains("其他账户登录") {
        return PageRoute::Login;
    }

    // Page-level navigation labels outrank status words that may also appear
    // on a home/service-card summary (for example, an already-completed card).
    if xml.contains("我的服务") {
        return PageRoute::Home;
    }
    if xml.contains("业务") {
        return PageRoute::Services;
    }

    // A status/action word alone is not sufficient page identity: the same
    // text can occur on summaries, overlays, and unrelated pages. The observed
    // check-in hierarchy exposes its page title, so require that context.
    let has_checkin_title = xml.contains("晚点名签到");
    let ordinary_checkin_markers = [
        "无法签到",
        "未到签到时间",
        "定位中",
        "点击签到",
        "签到成功",
        "已签到",
    ];
    let has_ordinary_checkin_marker = ordinary_checkin_markers
        .iter()
        .any(|marker| xml.contains(marker));
    let field_work_context_markers = ["未签到", "当前位置", "当前地点", "签到时间"];
    let field_work_context_count = field_work_context_markers
        .iter()
        .filter(|marker| xml.contains(**marker))
        .count();
    if has_checkin_title
        && (has_ordinary_checkin_marker
            || (xml.contains("外勤签到") && field_work_context_count >= 1))
    {
        return PageRoute::Checkin;
    }
    PageRoute::Unknown
}

fn is_navigable_catalog_route(route: PageRoute) -> bool {
    matches!(route, PageRoute::Home | PageRoute::Services)
}
fn locate_or_navigate(
    device: &adb::AdbDevice,
    profile: &profile::Profile,
    log: &RunLog,
) -> Result<NavigationResult> {
    let mut bytes = device.screenshot()?;
    let mut image = vision::decode(&bytes)?;
    let mut xml = device.dump_hierarchy()?;
    let initial_route = detect_page_route(&xml);
    if matches!(initial_route, PageRoute::Login | PageRoute::Checkin) {
        log.save_bytes("navigation_result.png", &bytes)?;
        let checkin_mode = detect_checkin_mode(&xml);
        return Ok(NavigationResult {
            route: initial_route,
            checkin_mode,
            field_work_ready: checkin_mode == CheckinMode::FieldWork
                && xml.contains("未签到")
                && xml.contains("外勤签到"),
        });
    }

    let min_entry_y = image.height() / 8;
    let mut navigated = false;

    // First use a hierarchy-confirmed entry only on a known home/services page.
    if matches!(initial_route, PageRoute::Home | PageRoute::Services)
        && let Some((x, y)) = find_clickable_labeled_bounds(&xml, "晚点名签到", min_entry_y)
    {
        device.tap(x, y)?;
        thread::sleep(Duration::from_secs(3));
        navigated = true;
        xml = device.dump_hierarchy()?;
    }

    // Some home layouts put the entry below the fold. Scroll only while the
    // hierarchy still identifies a known home/services page; never blind-tap a
    // service-icon coordinate as a fallback.
    if !navigated && matches!(initial_route, PageRoute::Home | PageRoute::Services) {
        for attempt in 0..4 {
            log.event("home_scroll", serde_json::json!({"attempt": attempt + 1}))?;
            device.swipe(
                image.width() / 2,
                image.height() * 3 / 4,
                image.width() * 2 / 5,
                image.height() * 2 / 5,
                500,
            )?;
            thread::sleep(Duration::from_secs(1));
            xml = device.dump_hierarchy()?;
            let route = detect_page_route(&xml);
            if !is_navigable_catalog_route(route) {
                break;
            }
            if let Some((x, y)) = find_clickable_labeled_bounds(&xml, "晚点名签到", min_entry_y)
            {
                device.tap(x, y)?;
                thread::sleep(Duration::from_secs(3));
                navigated = true;
                xml = device.dump_hierarchy()?;
                break;
            }
        }
    }

    // Landscape service catalogues can expose the entry only after opening the
    // explicitly labelled "业务" tab. Each navigation action is followed by a
    // fresh hierarchy read before any next action.
    if !navigated
        && matches!(
            detect_page_route(&xml),
            PageRoute::Home | PageRoute::Services
        )
        && let Some((x, y)) = find_clickable_labeled_bounds(&xml, "业务", 0)
    {
        device.tap(x, y)?;
        thread::sleep(Duration::from_secs(2));
        xml = device.dump_hierarchy()?;
        for attempt in 0..6 {
            if !is_navigable_catalog_route(detect_page_route(&xml)) {
                break;
            }
            if let Some((entry_x, entry_y)) =
                find_clickable_labeled_bounds(&xml, "晚点名签到", min_entry_y)
            {
                device.tap(entry_x, entry_y)?;
                thread::sleep(Duration::from_secs(3));
                xml = device.dump_hierarchy()?;
                break;
            }
            if attempt < 5 {
                let current_route = detect_page_route(&xml);
                if !matches!(current_route, PageRoute::Home | PageRoute::Services) {
                    break;
                }
                device.swipe(
                    image.width() / 2,
                    image.height() * 3 / 4,
                    image.width() / 2,
                    image.height() / 4,
                    500,
                )?;
                thread::sleep(Duration::from_secs(1));
                xml = device.dump_hierarchy()?;
            }
        }
    }

    bytes = device.screenshot()?;
    image = vision::decode(&bytes)?;
    let mut route = detect_page_route(&xml);
    let mut checkin_mode = if route == PageRoute::Checkin {
        detect_checkin_mode(&xml)
    } else {
        CheckinMode::Unsupported
    };
    if route == PageRoute::Checkin && checkin_mode == CheckinMode::Ordinary {
        // Ordinary check-in requires hierarchy evidence plus a visible button
        // candidate. If the button is below the fold, a bounded non-clicking
        // scroll is allowed only while route and mode remain confirmed.
        let mut visible_state = vision::ButtonState::Unknown;
        for attempt in 0..4 {
            let button = vision::find_colored_button(&image)
                .unwrap_or_else(|| vision::fallback_button(&image, profile));
            visible_state = vision::classify_button(&image, button).state;
            if visible_state != vision::ButtonState::Unknown || attempt == 3 {
                break;
            }
            device.swipe(
                image.width() / 2,
                image.height() * 3 / 4,
                image.width() / 2,
                image.height() / 4,
                500,
            )?;
            thread::sleep(Duration::from_secs(1));
            bytes = device.screenshot()?;
            image = vision::decode(&bytes)?;
            xml = device.dump_hierarchy()?;
            route = detect_page_route(&xml);
            checkin_mode = if route == PageRoute::Checkin {
                detect_checkin_mode(&xml)
            } else {
                CheckinMode::Unsupported
            };
            if route != PageRoute::Checkin || checkin_mode != CheckinMode::Ordinary {
                break;
            }
        }
        if visible_state == vision::ButtonState::Unknown {
            route = PageRoute::Unknown;
        }
    }
    log.save_bytes("navigation_result.png", &bytes)?;
    Ok(NavigationResult {
        route,
        checkin_mode,
        field_work_ready: checkin_mode == CheckinMode::FieldWork
            && xml.contains("未签到")
            && xml.contains("外勤签到"),
    })
}

#[allow(dead_code)]
fn find_labeled_bounds(xml: &str, label: &str, min_center_y: u32) -> Option<(u32, u32)> {
    find_labeled_bounds_internal(xml, label, min_center_y, false)
}

fn find_clickable_labeled_bounds(xml: &str, label: &str, min_center_y: u32) -> Option<(u32, u32)> {
    find_labeled_bounds_internal(xml, label, min_center_y, true)
}

fn find_labeled_bounds_internal(
    xml: &str,
    label: &str,
    min_center_y: u32,
    clickable_only: bool,
) -> Option<(u32, u32)> {
    for attribute in ["content-desc", "text"] {
        let prefix = format!("{}=\"", attribute);
        let mut cursor = 0;
        while let Some(relative) = xml[cursor..].find(&prefix) {
            let value_start = cursor + relative + prefix.len();
            let Some(value_length) = xml[value_start..].find('"') else {
                cursor = value_start.saturating_add(1);
                continue;
            };
            let value_end = value_start + value_length;
            let value = &xml[value_start..value_end];
            // A malformed attribute can swallow the start of the next node.
            // Resume scanning at that nested node instead of losing the rest
            // of the hierarchy dump.
            if let Some(next_node) = value.find("<node") {
                cursor = value_start + next_node + "<node".len();
                continue;
            }
            if value.contains(label) {
                let node_start = xml[..value_start].rfind("<node").unwrap_or(value_start);
                let Some(node_length) = xml[node_start..].find('>') else {
                    cursor = value_end + 1;
                    continue;
                };
                let node_end = node_start + node_length;
                let node = &xml[node_start..node_end];
                if clickable_only && !node.contains("clickable=\"true\"") {
                    cursor = value_end + 1;
                    continue;
                }
                if let Some(bounds_start) = node.find("bounds=\"") {
                    let bounds_value = &node[bounds_start + 8..];
                    if let Some(end) = bounds_value.find('"') {
                        let nums: Vec<u32> = bounds_value[..end]
                            .split(['[', ']', ','])
                            .filter_map(|s| s.parse().ok())
                            .collect();
                        if nums.len() >= 4 {
                            let center_y = (nums[1] + nums[3]) / 2;
                            if center_y >= min_center_y {
                                return Some(((nums[0] + nums[2]) / 2, center_y));
                            }
                        }
                    }
                }
            }
            cursor = value_end + 1;
        }
    }
    None
}

fn print_device(serial: &str, info: &adb::DeviceInfo) {
    println!(
        "serial={} model={} sdk={} size={}x{} dpi={} orientation={} package={:?}",
        serial,
        info.model,
        info.sdk,
        info.width,
        info.height,
        info.density_dpi,
        info.orientation.as_str(),
        info.package
    );
}

#[cfg(test)]
mod tests {
    use super::{
        PageRoute, SyncLifecycle, TapDevice, TapRequest, build_task_command_line,
        detect_page_route, find_clickable_labeled_bounds, find_labeled_bounds, guarded_tap,
        restore_sync_backup,
    };
    use crate::{
        domain::{CheckinMode, Mode},
        vision::ButtonState,
    };
    use anyhow::Result;
    use std::{
        cell::{Cell, RefCell},
        fs,
        path::Path,
        time::Duration,
    };

    #[test]
    fn labeled_bounds_match_accessibility_suffix_and_skip_header() {
        let xml = r#"<node content-desc="晚点名签到" bounds="[10,20][30,40]"/><node content-desc="业务&#10;第 4 个标签，共 4 个" bounds="[100,600][300,700]"/>"#;
        assert_eq!(find_labeled_bounds(xml, "晚点名签到", 100), None);
        assert_eq!(find_labeled_bounds(xml, "业务", 100), Some((200, 650)));
        let clickable =
            r#"<node content-desc="晚点名签到" clickable="true" bounds="[10,200][30,240]"/>"#;
        assert_eq!(
            find_clickable_labeled_bounds(clickable, "晚点名签到", 100),
            Some((20, 220))
        );
        let malformed_then_valid = r#"<node content-desc="broken><node content-desc="晚点名签到" clickable="true" bounds="[10,200][30,240]"/>"#;
        assert_eq!(
            find_clickable_labeled_bounds(malformed_then_valid, "晚点名签到", 100),
            Some((20, 220))
        );
    }

    #[test]
    fn page_route_detection_rejects_login_and_home_as_checkin() {
        assert_eq!(
            detect_page_route(r#"<node content-desc="一键登录"/>"#),
            PageRoute::Login
        );
        assert_eq!(
            detect_page_route(r#"<node content-desc="我的服务"/>"#),
            PageRoute::Home
        );
        assert_eq!(
            detect_page_route(r#"<node text="晚点名签到"/><node text="无法签到"/>"#),
            PageRoute::Checkin
        );
        assert_eq!(
            detect_page_route(
                r#"<node text="晚点名签到"/><node text="未签到"/><node text="外勤签到"/><node text="当前位置"/>"#
            ),
            PageRoute::Checkin
        );
        assert_eq!(
            detect_page_route(r#"<node text="我的服务"/><node text="外勤签到"/>"#),
            PageRoute::Home
        );
        assert_eq!(detect_page_route("<hierarchy />"), PageRoute::Unknown);
    }

    #[test]
    fn page_route_prioritizes_home_and_services_over_checkin_card_status_words() {
        assert_eq!(
            detect_page_route(
                r#"<node text="我的服务"/><node text="晚点名签到"/><node text="已签到"/>"#
            ),
            PageRoute::Home
        );
        assert_eq!(
            detect_page_route(r#"<node text="业务"/><node text="签到成功"/>"#),
            PageRoute::Services
        );
        assert!(super::is_navigable_catalog_route(PageRoute::Home));
        assert!(super::is_navigable_catalog_route(PageRoute::Services));
        assert!(!super::is_navigable_catalog_route(PageRoute::Unknown));
        assert!(!super::is_navigable_catalog_route(PageRoute::Login));
        assert!(!super::is_navigable_catalog_route(PageRoute::Checkin));
    }
    #[test]
    fn checkin_action_or_success_words_without_page_title_remain_unknown() {
        assert_eq!(
            detect_page_route(r#"<node text="点击签到"/>"#),
            PageRoute::Unknown
        );
        assert_eq!(
            detect_page_route(r#"<node text="已签到"/>"#),
            PageRoute::Unknown
        );
        assert_eq!(
            detect_page_route(
                r#"<node text="外勤签到"/><node text="未签到"/><node text="当前位置"/>"#
            ),
            PageRoute::Unknown
        );
    }
    #[test]
    fn checkin_mode_is_distinct_from_visual_color() {
        assert_eq!(
            super::detect_checkin_mode(
                r#"<node text="晚点名签到"/><node text="未签到"/><node text="外勤签到"/>"#
            ),
            CheckinMode::FieldWork
        );
        assert_eq!(
            super::detect_checkin_mode(r#"<node text="无法签到"/><node text="点击签到"/>"#),
            CheckinMode::Ordinary
        );
        assert_eq!(
            super::detect_checkin_mode("<hierarchy />"),
            CheckinMode::Unsupported
        );
    }

    #[test]
    fn success_date_parser_supports_chinese_and_numeric_formats() {
        assert_eq!(
            super::extract_hierarchy_date(r#"<node text="签到成功：2026年10月10日"/>"#),
            chrono::NaiveDate::from_ymd_opt(2026, 10, 10)
        );
        assert_eq!(
            super::extract_hierarchy_date(r#"<node text="日期 2026-10-10"/>"#),
            chrono::NaiveDate::from_ymd_opt(2026, 10, 10)
        );
        assert_eq!(
            super::extract_hierarchy_date(r#"<node text="没有日期"/>"#),
            None
        );
    }

    #[test]
    fn unrelated_update_date_is_not_accepted_as_success_date() {
        let hierarchy = r#"<node text="签到成功"/><node text="上次更新 2026-10-11"/>"#;
        assert_eq!(super::extract_hierarchy_date(hierarchy), None);
    }
    #[test]
    fn hierarchy_date_is_unknown_when_success_and_unrelated_dates_conflict() {
        let hierarchy = r#"<node text="2026年10月11日"/><node text="签到成功：2026年10月10日"/>"#;
        assert_eq!(super::extract_hierarchy_date(hierarchy), None);
    }
    #[test]
    fn gui_task_creation_targets_cli_companion() {
        assert_eq!(
            super::cli_executable_path(Path::new(r"C:\Program Files\ZHFD\zhfd-checkin-gui.exe",)),
            Path::new(r"C:\Program Files\ZHFD\zhfd-checkin.exe")
        );
        assert_eq!(
            super::cli_executable_path(Path::new(r"C:\ZHFD\zhfd-checkin.exe")),
            Path::new(r"C:\ZHFD\zhfd-checkin.exe")
        );
    }

    #[test]
    fn task_command_line_quotes_exe_path_and_targets_instance() {
        let target = super::InstanceTargetArgs {
            instance_index: Some(1),
            serial: None,
        };
        let command = build_task_command_line(
            Path::new(r"C:\Program Files\ZHFD\zhfd-checkin.exe"),
            "run --dry-run",
            false,
            &target,
        );
        assert_eq!(
            command,
            r#""C:\Program Files\ZHFD\zhfd-checkin.exe" run --dry-run --instance-index 1"#
        );
    }

    #[derive(Default)]
    struct MockTapDevice {
        taps: RefCell<Vec<(u32, u32)>>,
    }

    impl TapDevice for MockTapDevice {
        fn tap_at(&self, x: u32, y: u32) -> Result<()> {
            self.taps.borrow_mut().push((x, y));
            Ok(())
        }
    }

    fn request(state: ButtonState) -> TapRequest {
        TapRequest {
            state,
            route: PageRoute::Checkin,
            checkin_mode: CheckinMode::Ordinary,
            mode: Mode::Live,
            explicit_confirmation: true,
            profile_calibrated: true,
            foreground_matches: true,
            within_window: true,
            ready_frames: 2,
            required_frames: 2,
            x: 448,
            y: 779,
        }
    }

    #[test]
    fn guarded_tap_is_fail_closed_for_non_ready_states_and_bad_policy() {
        let device = MockTapDevice::default();
        for state in [
            ButtonState::Gray,
            ButtonState::Locating,
            ButtonState::Unknown,
        ] {
            assert!(!guarded_tap(&device, request(state)).unwrap());
        }

        let mut field_work = request(ButtonState::Ready);
        field_work.checkin_mode = CheckinMode::FieldWork;
        assert!(guarded_tap(&device, field_work).is_err());

        let mut wrong_route = request(ButtonState::Ready);
        wrong_route.route = PageRoute::Home;
        assert!(guarded_tap(&device, wrong_route).is_err());

        let mut not_stable = request(ButtonState::Ready);
        not_stable.ready_frames = 1;
        assert!(guarded_tap(&device, not_stable).is_err());

        let mut uncalibrated = request(ButtonState::Ready);
        uncalibrated.profile_calibrated = false;
        assert!(guarded_tap(&device, uncalibrated).is_err());

        let mut wrong_foreground = request(ButtonState::Ready);
        wrong_foreground.foreground_matches = false;
        assert!(guarded_tap(&device, wrong_foreground).is_err());

        let mut outside_window = request(ButtonState::Ready);
        outside_window.within_window = false;
        assert!(guarded_tap(&device, outside_window).is_err());

        assert!(device.taps.borrow().is_empty());
    }

    #[test]
    fn guarded_tap_calls_mock_once_at_confirmed_ready_coordinate() {
        let device = MockTapDevice::default();
        assert!(guarded_tap(&device, request(ButtonState::Ready)).unwrap());
        assert_eq!(*device.taps.borrow(), vec![(448, 779)]);
    }

    struct MockSyncLifecycle {
        running: Cell<bool>,
        stops: Cell<u32>,
        launches: Cell<u32>,
        fail_launch: bool,
    }

    impl MockSyncLifecycle {
        fn new(running: bool, fail_launch: bool) -> Self {
            Self {
                running: Cell::new(running),
                stops: Cell::new(0),
                launches: Cell::new(0),
                fail_launch,
            }
        }
    }

    impl SyncLifecycle for MockSyncLifecycle {
        fn is_running(&self, _index: u32) -> Result<bool> {
            Ok(self.running.get())
        }

        fn stop_and_wait(&self, _index: u32) -> Result<()> {
            self.stops.set(self.stops.get() + 1);
            self.running.set(false);
            Ok(())
        }

        fn launch_wait(&self, _index: u32, _timeout: Duration) -> Result<()> {
            self.launches.set(self.launches.get() + 1);
            if self.fail_launch {
                anyhow::bail!("mock launch failure");
            }
            self.running.set(true);
            Ok(())
        }
    }

    fn temp_recovery_paths() -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "zhfd-sync-recovery-{}-{}",
            std::process::id(),
            nonce
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        (
            root.clone(),
            root.join("source.config"),
            root.join("backup.config"),
        )
    }

    #[test]
    fn sync_recovery_restores_backup_and_original_running_state() {
        let (root, source, backup) = temp_recovery_paths();
        fs::write(&source, b"modified").unwrap();
        fs::write(&backup, b"original").unwrap();
        let controller = MockSyncLifecycle::new(true, false);

        restore_sync_backup(
            &controller,
            0,
            Some(Path::new(&source)),
            Some(Path::new(&backup)),
            true,
            Duration::from_secs(5),
        )
        .unwrap();

        assert_eq!(fs::read(&source).unwrap(), b"original");
        assert_eq!(controller.stops.get(), 1);
        assert_eq!(controller.launches.get(), 1);
        assert!(controller.running.get());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sync_recovery_reports_restart_failure_after_restoring_file() {
        let (root, source, backup) = temp_recovery_paths();
        fs::write(&source, b"modified").unwrap();
        fs::write(&backup, b"original").unwrap();
        let controller = MockSyncLifecycle::new(true, true);

        let error = restore_sync_backup(
            &controller,
            0,
            Some(Path::new(&source)),
            Some(Path::new(&backup)),
            true,
            Duration::from_secs(5),
        )
        .unwrap_err();

        assert!(error.to_string().contains("mock launch failure"));
        assert_eq!(fs::read(&source).unwrap(), b"original");
        assert_eq!(controller.stops.get(), 1);
        assert_eq!(controller.launches.get(), 1);
        let _ = fs::remove_dir_all(root);
    }
}
