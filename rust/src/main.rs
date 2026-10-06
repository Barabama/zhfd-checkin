mod adb;
mod apk;
mod config;
mod domain;
mod gui;
mod ldplayer;
mod logger;
mod profile;
mod vision;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use config::ConfigStore;
use domain::{ExitCode, Mode, RunResult};
use logger::{RunLog, SyncGeometry, SyncLog, SyncResult};
use std::{
    path::{Path, PathBuf},
    process::Command,
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
            let default_serial = ldplayer::LdPlayer::serial_for_index(instance.index);
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
        .unwrap_or_else(|| ldplayer::LdPlayer::serial_for_index(index));
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
    pub adb_port: Option<u16>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub dpi: Option<u32>,
    pub configured: bool,
    pub enabled: bool,
}

pub(crate) fn list_instance_summaries(store: &ConfigStore) -> Result<Vec<InstanceSummary>> {
    let ld = ldplayer::LdPlayer::discover(&store.config.emulator.ldconsole_path, &store.root)?;
    let configured_instances = store.config.emulator.effective_instances();
    Ok(ld
        .instances()?
        .into_iter()
        .map(|instance| {
            let configured = configured_instances
                .iter()
                .find(|configured| configured.instance_index == instance.index);
            InstanceSummary {
                index: instance.index,
                name: configured
                    .filter(|item| !item.name.trim().is_empty())
                    .map(|item| item.name.clone())
                    .unwrap_or(instance.name),
                running: instance.running,
                serial: configured
                    .filter(|item| !item.serial.trim().is_empty())
                    .map(|item| item.serial.clone())
                    .unwrap_or_else(|| ldplayer::LdPlayer::serial_for_index(instance.index)),
                adb_port: instance.adb_port,
                width: instance.width,
                height: instance.height,
                dpi: instance.dpi,
                configured: configured.is_some(),
                enabled: configured.is_some_and(|item| item.enabled),
            }
        })
        .collect())
}

fn instance_command(store: &ConfigStore, command: InstanceCommand) -> Result<ExitCode> {
    match command {
        InstanceCommand::List => {
            for instance in list_instance_summaries(store)? {
                println!(
                    "index={} name={} running={} serial={} adb_port={:?} size={:?}x{:?} dpi={:?} configured={} enabled={}",
                    instance.index,
                    instance.name,
                    instance.running,
                    instance.serial,
                    instance.adb_port,
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
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        Command::new(&gui_exe)
            .creation_flags(DETACHED_PROCESS)
            .spawn()
            .with_context(|| format!("无法启动 GUI: {}", gui_exe.display()))?;
    }
    #[cfg(not(windows))]
    {
        Command::new(&gui_exe)
            .spawn()
            .with_context(|| format!("无法启动 GUI: {}", gui_exe.display()))?;
    }
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
                Command::new("cmd")
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
    let ld = ldplayer::LdPlayer::discover(&store.config.emulator.ldconsole_path, &store.root)?;
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

fn diagnose(store: &ConfigStore, instance_target: &InstanceTargetArgs) -> Result<ExitCode> {
    let ld = ldplayer::LdPlayer::discover(&store.config.emulator.ldconsole_path, &store.root)?;
    println!("LDPlayer: {}", ld.executable.display());
    let instances = ld.instances()?;
    if instances.is_empty() {
        println!("实例: none");
        return Ok(ExitCode::DeviceFailure);
    }
    for i in &instances {
        println!(
            "instance={} name={} running={} adb_port={:?} size={:?}x{:?} dpi={:?}",
            i.index, i.name, i.running, i.adb_port, i.width, i.height, i.dpi
        );
    }
    let (_, device, info) = runtime_with_target(store, false, Some(instance_target))?;
    print_device(&device.serial, &info);
    if let Some(p) = profile::find_profile(info.width, info.height, info.density_dpi) {
        println!("profile={} calibrated={}", p.id, p.calibrated);
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
            let output = Command::new("schtasks.exe")
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
            let output = Command::new("schtasks.exe")
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
            let output = Command::new("schtasks.exe")
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
        mode: mode.as_str().into(),
        instance_index: args.target.instance_index,
        instance_name: None,
        serial: String::new(),
        profile_id: None,
        state_history: vec![],
        clicked: false,
        success: false,
        dry_run_ready: false,
        error: None,
    };
    log.event("start", serde_json::json!({"mode":mode.as_str()}))?;
    if mode == Mode::Live && !args.confirm {
        result.error = Some("live_confirmation_required".into());
        finalize_run(&log, &result)?;
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
            eprintln!("运行失败: {:#}", error);
            ExitCode::DeviceFailure
        }
    };

    finalize_run(&log, &result)?;
    println!("结果: {}", serde_json::to_string_pretty(&result)?);
    Ok(code)
}

fn finalize_run(log: &RunLog, result: &RunResult) -> Result<()> {
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
    run_inner_with_device(store, args, mode, log, result, |store, launch, target| {
        runtime_with_target(store, launch, Some(target))
    })
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
    ) -> Result<(ldplayer::LdPlayer, adb::AdbDevice, adb::DeviceInfo)>,
{
    let (ld, device, info) = runtime_provider(store, true, &args.target)?;
    result.serial = device.serial.clone();
    let selected = resolve_instance_target(&ld, store, Some(&args.target))?;
    result.instance_index = Some(selected.index);
    result.instance_name = Some(selected.name);
    let profile = match profile::find_profile(info.width, info.height, info.density_dpi) {
        Some(p) => p,
        None => {
            result.error = Some("unknown_profile".into());
            log.event(
                "profile_unknown",
                serde_json::json!({"width":info.width,"height":info.height,"dpi":info.density_dpi}),
            )?;
            return Ok(ExitCode::VisionFailure);
        }
    };
    result.profile_id = Some(profile.id.to_string());
    if mode == Mode::Live && !profile.calibrated {
        result.error = Some("uncalibrated_profile".into());
        return Ok(ExitCode::BusinessFailure);
    }
    device.launch_package(
        &store.config.app.package_name,
        &store.config.app.activity_name,
    )?;
    wait_for_package(
        &device,
        &store.config.app.package_name,
        Duration::from_secs(store.config.runtime.startup_timeout_seconds),
    )?;
    thread::sleep(Duration::from_millis(800));
    let _initial_button = locate_or_navigate(&device, profile, log)?;
    let started = Instant::now();
    let mut unknowns = 0u32;
    let mut ready_frames = 0u32;
    let mut click_deadline: Option<Instant> = None;
    loop {
        if click_deadline.is_none() {
            let within_window = domain::in_configured_window(
                &store.config.window.start,
                &store.config.window.end,
                &store.config.window.timezone,
            )?;
            if !within_window {
                result.error = Some("outside_window".into());
                break;
            }
            if started.elapsed()
                >= Duration::from_secs(store.config.runtime.location_timeout_seconds)
            {
                result.error = Some("location_timeout".into());
                break;
            }
        }

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
        log.event("state", &analysis)?;
        if analysis.state == vision::ButtonState::Success {
            result.success = true;
            break;
        }
        if let Some(deadline) = click_deadline {
            if deadline.elapsed()
                > Duration::from_secs(store.config.runtime.success_timeout_seconds)
            {
                result.error = Some("success_timeout".into());
                break;
            }
            // A delayed UI frame may still look ready after the tap. Once a
            // tap has happened, only wait for success; never tap again.
            thread::sleep(Duration::from_secs(3));
            continue;
        }
        match analysis.state {
            vision::ButtonState::Gray => {
                ready_frames = 0;
                sleep_for_next_poll(store, started)?;
            }
            vision::ButtonState::Locating => {
                ready_frames = 0;
                sleep_for_next_poll(store, started)?;
            }
            vision::ButtonState::Ready => {
                let within_window = domain::in_configured_window(
                    &store.config.window.start,
                    &store.config.window.end,
                    &store.config.window.timezone,
                )?;
                if !within_window {
                    result.error = Some("outside_window".into());
                    break;
                }
                ready_frames += 1;
                if ready_frames < store.config.runtime.stable_frames {
                    thread::sleep(Duration::from_millis(700));
                    continue;
                }
                if mode == Mode::DryRun {
                    result.dry_run_ready = true;
                    break;
                }
                let foreground_matches =
                    device.current_package().unwrap_or_default() == store.config.app.package_name;
                if !foreground_matches {
                    result.error = Some("foreground_package_changed".into());
                    break;
                }
                let confirm_bytes = device.screenshot()?;
                let confirm_image = vision::decode(&confirm_bytes)?;
                let confirm_button = vision::find_colored_button(&confirm_image).unwrap_or(button);
                let confirm = vision::classify_button(&confirm_image, confirm_button);
                if confirm.state != vision::ButtonState::Ready {
                    result.error = Some("ready_confirmation_failed".into());
                    break;
                }
                let policy_window = domain::in_configured_window(
                    &store.config.window.start,
                    &store.config.window.end,
                    &store.config.window.timezone,
                )?;
                match guarded_tap(
                    &device,
                    TapRequest {
                        state: confirm.state,
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
                        result.error = Some("ready_confirmation_failed".into());
                        break;
                    }
                    Err(policy_error) => {
                        result.error = Some(policy_error.to_string());
                        break;
                    }
                }
                result.clicked = true;
                log.event(
                    "clicked_ready",
                    serde_json::json!({"x":confirm_button.cx,"y":confirm_button.cy}),
                )?;
                click_deadline = Some(Instant::now());
            }
            vision::ButtonState::Success => {
                result.success = true;
                break;
            }
            vision::ButtonState::Unknown => {
                unknowns += 1;
                ready_frames = 0;
                if unknowns >= store.config.runtime.unknown_retries {
                    result.error = Some("unknown_state".into());
                    break;
                }
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
    let code = if result.success || result.dry_run_ready {
        ExitCode::Ok
    } else if result.error.as_deref() == Some("location_timeout")
        || result.error.as_deref() == Some("success_timeout")
    {
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
    let ld = ldplayer::LdPlayer::discover(&store.config.emulator.ldconsole_path, &store.root)?;
    let selected = resolve_instance_target(&ld, store, instance_target)?;
    let instance = ld.instance(selected.index)?;
    if launch && store.config.emulator.auto_launch && !instance.running {
        ld.launch_wait(
            instance.index,
            Duration::from_secs(store.config.runtime.startup_timeout_seconds),
        )?;
    }
    let adb_path = adb::find_adb(&store.config.emulator.adb_path)?;
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
        let connect_error = match Command::new(adb_path).args(["connect", serial]).output() {
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

fn locate_or_navigate(
    device: &adb::AdbDevice,
    profile: &profile::Profile,
    log: &RunLog,
) -> Result<vision::ButtonBox> {
    let bytes = device.screenshot()?;
    let image = vision::decode(&bytes)?;

    // A home-page service tile can contain saturated colors and white text that
    // look like a locating button to the visual detector. Trust the
    // profile-specific fallback first, then use clickable accessibility labels
    // to navigate, and only use the dynamic detector after the page route is
    // known.
    let fallback = vision::fallback_button(&image, profile);
    if vision::classify_button(&image, fallback).state != vision::ButtonState::Unknown {
        return Ok(fallback);
    }

    let mut xml = device.dump_hierarchy().unwrap_or_default();
    let min_entry_y = image.height() / 8;
    let mut navigated = false;

    // 1. The check-in entry may already be a clickable node on the current page.
    if let Some((x, y)) = find_clickable_labeled_bounds(&xml, "晚点名签到", min_entry_y) {
        device.tap(x, y)?;
        thread::sleep(Duration::from_secs(3));
        navigated = true;
    }

    // 2. Some layouts keep the check-in tile below the fold on the home page,
    //    so scroll the current page and rescan before trying other routes.
    //    Scrolling a check-in page cannot activate the sign-in control.
    if !navigated {
        for attempt in 0..4 {
            log.event("home_scroll", serde_json::json!({"attempt": attempt + 1}))?;
            device.swipe(
                image.width() / 2,
                image.height() * 3 / 4,
                image.width() / 2,
                image.height() * 2 / 5,
                500,
            )?;
            thread::sleep(Duration::from_secs(1));
            xml = device.dump_hierarchy().unwrap_or_default();
            if let Some((x, y)) = find_clickable_labeled_bounds(&xml, "晚点名签到", min_entry_y)
            {
                device.tap(x, y)?;
                thread::sleep(Duration::from_secs(3));
                navigated = true;
                break;
            }
        }
    }

    // 3. Landscape layouts hide the service catalogue behind the "业务" tab and
    //    may require one or more vertical swipes before the check-in tile
    //    becomes part of the accessibility hierarchy.
    if !navigated && let Some((x, y)) = find_clickable_labeled_bounds(&xml, "业务", 0) {
        device.tap(x, y)?;
        thread::sleep(Duration::from_secs(2));
        for attempt in 0..6 {
            let page_xml = device.dump_hierarchy().unwrap_or_default();
            if let Some((entry_x, entry_y)) =
                find_clickable_labeled_bounds(&page_xml, "晚点名签到", min_entry_y)
            {
                device.tap(entry_x, entry_y)?;
                thread::sleep(Duration::from_secs(3));
                navigated = true;
                break;
            }
            if attempt < 5 {
                device.swipe(
                    image.width() / 2,
                    image.height() * 3 / 4,
                    image.width() / 2,
                    image.height() / 4,
                    500,
                )?;
                thread::sleep(Duration::from_secs(1));
            }
        }
    }

    // 4. Legacy positional fallback when no accessibility entry was found.
    if !navigated {
        device.tap(
            (profile.service_icon_center_ratio.0 * image.width() as f32) as u32,
            (profile.service_icon_center_ratio.1 * image.height() as f32) as u32,
        )?;
        thread::sleep(Duration::from_secs(3));
    }

    let mut bytes = device.screenshot()?;
    let mut image = vision::decode(&bytes)?;
    for attempt in 0..4 {
        if let Some(button) = vision::find_colored_button(&image) {
            let analysis = vision::classify_button(&image, button);
            if analysis.state != vision::ButtonState::Unknown {
                log.save_bytes("navigation_result.png", &bytes)?;
                return Ok(button);
            }
        }
        let fallback = vision::fallback_button(&image, profile);
        if vision::classify_button(&image, fallback).state != vision::ButtonState::Unknown {
            log.save_bytes("navigation_result.png", &bytes)?;
            return Ok(fallback);
        }
        if !navigated || attempt == 3 {
            break;
        }
        // Some landscape WebView layouts render the check-in circle below the
        // initial viewport. Scroll only after selecting the check-in tile;
        // this cannot activate the sign-in control.
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
    }
    log.save_bytes("navigation_result.png", &bytes)?;
    Ok(vision::fallback_button(&image, profile))
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
        SyncLifecycle, TapDevice, TapRequest, build_task_command_line,
        find_clickable_labeled_bounds, find_labeled_bounds, guarded_tap, restore_sync_backup,
    };
    use crate::{domain::Mode, vision::ButtonState};
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
