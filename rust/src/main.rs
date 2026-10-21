mod adb;
mod apk;
mod auto_login;
mod cli;
mod commands;
mod config;
mod discovery;
mod domain;
mod foreground;
mod gui;
mod ldplayer;
mod logger;
mod page;
mod process;
mod profile;
mod vision;

use anyhow::{Context, Result, bail};
use cli::{
    AppCommand, Cli, CliConfigCommand, CommandKind, InstanceCommand, LoginArgs, ProfileCommand,
    UiCommand,
};
use commands::{ConfigRequest, InstanceTargetArgs, RunPolicy, RunRequest, RunSource, TaskRequest};
use config::ConfigStore;
use domain::{
    CheckinMode, CheckinState, ConsecutiveUnknown, ExitCode, Mode, PageRoute, RunResult, RunStatus,
    set_run_failure,
};
#[cfg(test)]
use foreground::foreground_check;
use foreground::{log_foreground_snapshot, require_target_foreground};
use logger::{RunLog, SyncGeometry, SyncLog, SyncResult};
#[cfg(test)]
use page::detect_checkin_mode;
use page::{
    CHECKIN_ENTRY_LABEL, PageObservation, PageView, attribute_value, detect_page_route,
    observe_page_route, tag_body_end,
};
use std::{
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

const LDPLAYER_URL: &str = "https://www.ldmnq.com/";
const ZHFD_URL: &str = "https://app.fzu.edu.cn/fd-app/m/index.html";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GIT_COMMIT: &str = match option_env!("ZHFD_GIT_COMMIT") {
    Some(value) => value,
    None => "unknown",
};
/// `Some(true)` when the tree this binary was built from had uncommitted
/// tracked changes, `Some(false)` when it was clean, `None` when the build
/// could not determine it. `git_commit` on its own would otherwise claim a
/// provenance the binary does not have (FR-014).
fn git_dirty() -> Option<bool> {
    match option_env!("ZHFD_GIT_DIRTY") {
        Some("1") => Some(true),
        Some("0") => Some(false),
        _ => None,
    }
}
/// Human-readable provenance suffix: a dirty build is marked so nobody reads
/// `build_id` as an exact commit reference.
fn build_id() -> String {
    build_id_for(VERSION, GIT_COMMIT, git_dirty())
}

fn build_id_for(version: &str, commit: &str, dirty: Option<bool>) -> String {
    match dirty {
        Some(true) => format!("{version}-{commit}+dirty"),
        Some(false) => format!("{version}-{commit}"),
        None => format!("{version}-{commit}+unknown"),
    }
}

/// CLI entry point. `pub` on purpose: the library target shares this file via
/// `include!`, and a private `main` would make every CLI-only function look
/// dead there -- which is what previously forced `#[allow(dead_code)]` blankets
/// over production code paths.
pub fn main() {
    // Parse first so a malformed command line still produces a clap usage error
    // instead of being flattened into the generic config failure path.
    let cli = Cli::parse_args();
    let code = match run_cli(cli) {
        Ok(code) => code as i32,
        Err(error) => {
            eprintln!("错误: {:#}", error);
            ExitCode::ConfigFailure as i32
        }
    };
    std::process::exit(code);
}

fn run_cli(cli: Cli) -> Result<ExitCode> {
    let config_path = cli.config;
    match cli.command.unwrap_or(CommandKind::Gui) {
        CommandKind::About => {
            print_about();
            Ok(ExitCode::Ok)
        }
        // `gui` must never be blocked by a broken config: the GUI is the only
        // Repair entry point, so it launches and shows the recovery page itself.
        CommandKind::Gui => launch_gui_companion(config_path.as_deref()),
        // Pure local commands do not need a config and must stay usable while
        // the config is damaged (troubleshooting/repair path).
        CommandKind::Vision { image } => analyze_image(&image),
        CommandKind::Profile {
            command: ProfileCommand::List,
        } => profile_list(),
        CommandKind::Config {
            command: CliConfigCommand::Path,
        } => config_path_command(config_path.as_deref()),
        CommandKind::Task { command } => task_command(command.into(), config_path.as_deref()),
        command => {
            let store = load_store_for_cli(config_path.as_deref())?;
            match command {
                CommandKind::About
                | CommandKind::Gui
                | CommandKind::Vision { .. }
                | CommandKind::Task { .. } => unreachable!(),
                CommandKind::Profile { command } => profile_command(&store, command),
                CommandKind::Instance { command } => instance_command(&store, command),
                CommandKind::Ui { command } => ui_command(&store, command),
                CommandKind::Config { command } => config_command(&store, command.into()),
                CommandKind::Diagnose { target } => diagnose(&store, &target.into()),
                CommandKind::Report => write_report(&store),
                CommandKind::App { command } => app_command(&store, command),
                CommandKind::Run(args) => run(&store, args.into_request(), RunSource::Cli),
                CommandKind::Login(args) => login_command(&store, args),
            }
        }
    }
}

/// Load the config for a non-GUI command without ever repairing or overwriting
/// it. A damaged or unreadable config is reported with its exact path and a
/// pointer at the GUI Repair entry (frozen decision: no silent fallback, no
/// reset, no overwrite). CLI deliberately does not grow a repair command in
/// v0.3.0 -- see `V0.3.0_DECISIONS.md` D-01/D-03.
fn load_store_for_cli(config_path: Option<&Path>) -> Result<ConfigStore> {
    ConfigStore::load_with_config_path(config_path).map_err(|error| {
        let path = describe_config_path(config_path);
        anyhow::anyhow!(
            "无法加载配置：{:#}\n配置文件：{path}\n\
             该命令不会自动恢复默认配置，也不会覆盖现有文件。\n\
             请运行 zhfd-checkin-gui.exe 进入 Setup/Repair 页面修复配置后重试。",
            error
        )
    })
}

/// Report a device/ADB failure at a command boundary and return the stable
/// device exit code instead of letting it fall through to the generic
/// config/unknown code.
fn reported_device_failure(error: anyhow::Error) -> ExitCode {
    eprintln!("错误: {error:#}");
    ExitCode::DeviceFailure
}

/// Best-effort display path for a config that failed to load: the explicit
/// `--config` value when given, otherwise the portable candidate that
/// `ConfigStore::load_with_config_path` prefers.
fn describe_config_path(config_path: Option<&Path>) -> String {
    if let Some(path) = config_path {
        return path.display().to_string();
    }
    // Must match the path the loader actually tried, or the message would name
    // a different file than the one that failed.
    ConfigStore::default_config_path()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "config.toml (无法确定配置路径)".into())
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

/// Validate a user-entered dependency selection before it is persisted.
///
/// The Setup page calls this so that "tools verified" is a distinct fact from
/// "instances discovered" and from "ADB device online"; the returned summary
/// reports each stage separately (requirements FR-002/FR-003). Nothing here
/// taps the App or the check-in control.
pub(crate) fn validate_dependency_selection(
    store: &ConfigStore,
    ldconsole_path: &str,
    adb_path: &str,
) -> Result<String> {
    let ld = ldplayer::LdPlayer::discover(ldconsole_path, &store.root)
        .map_err(|error| anyhow::anyhow!("LDPlayer 工具验证失败: {:#}", error))?;
    let instances = ld
        .instances()
        .map_err(|error| anyhow::anyhow!("ldconsole list2 读取失败: {:#}", error))?;
    if instances.is_empty() {
        bail!("ldconsole list2 未发现任何实例;请确认 LDPlayer 已安装并至少创建一个实例");
    }
    let adb_path = adb::find_adb_for_ldplayer(adb_path, Some(&ld.executable), Some(&store.root))
        .map_err(|error| anyhow::anyhow!("ADB 工具验证失败: {:#}", error))?;
    let output = process::command(&adb_path)
        .arg("version")
        .output()
        .with_context(|| format!("无法执行 ADB: {}", adb_path.display()))?;
    if !output.status.success() {
        bail!(
            "ADB version 失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(format!(
        "工具已验证;LDPlayer:{} ({} 个实例);ADB:{};版本:{}",
        ld.executable.display(),
        instances.len(),
        adb_path.display(),
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or("unknown")
    ))
}

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
    pub profile_id: Option<String>,
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
                profile_id: instance
                    .width
                    .zip(instance.height)
                    .zip(instance.dpi)
                    .and_then(|((width, height), dpi)| {
                        profile::find_profile(width, height, dpi).map(|item| item.id.to_string())
                    }),
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

/// GUI startup entry. A missing or damaged config must land the user in the
/// Repair page rather than a startup error dialog, so a load failure is turned
/// into a `ConfigStore::recovery` handle that keeps the broken file untouched.
pub fn launch_gui() -> anyhow::Result<()> {
    let mut args = std::env::args_os();
    let _exe = args.next();
    let mut config_path = None;
    while let Some(arg) = args.next() {
        if arg == "about" || arg == "--about" {
            print_about();
            return Ok(());
        }
        if arg == "--config" {
            config_path = args.next().map(PathBuf::from);
        }
    }
    let (store, startup_error) = match ConfigStore::load_with_config_path(config_path.as_deref()) {
        Ok(store) => (store, None),
        Err(error) => {
            let message = format!("{error:#}");
            (
                ConfigStore::recovery(config_path.as_deref(), message.clone())?,
                Some(message),
            )
        }
    };
    gui::launch(store, startup_error)
}

fn launch_gui_companion(config_path: Option<&Path>) -> Result<ExitCode> {
    let cli_exe = std::env::current_exe().context("无法取得 CLI EXE 路径")?;
    let gui_exe = cli_exe
        .parent()
        .context("CLI EXE 没有父目录")?
        .join("zhfd-checkin-gui.exe");
    if !gui_exe.is_file() {
        bail!(
            "找不到独立 GUI 程序：{}；请运行 zhfd-checkin-gui.exe",
            gui_exe.display()
        );
    }
    // The GUI is the Repair entry point, so an explicit config path must be
    // forwarded verbatim; otherwise repairing a non-default config would show
    // the portable candidate instead.
    let mut command = process::detached_command(&gui_exe);
    if let Some(path) = config_path {
        command.arg("--config").arg(path);
    }
    command
        .spawn()
        .with_context(|| format!("无法启动 GUI: {}", gui_exe.display()))?;
    Ok(ExitCode::Ok)
}

fn write_report(store: &ConfigStore) -> Result<ExitCode> {
    let path = logger::write_diagnostic_report(store)?;
    println!("诊断报告: {}", path.display());
    Ok(ExitCode::Ok)
}

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

fn print_about() {
    println!("zhfd-checkin {}", VERSION);
    println!("Git commit: {}", GIT_COMMIT);
    println!("Build id: {}", build_id());
    // Provenance honesty (FR-014): a build from a tree with uncommitted tracked
    // changes is not the commit it names, and must say so.
    match git_dirty() {
        Some(true) => println!("工作区状态: dirty（包含未提交的已跟踪改动，非纯提交构建）"),
        Some(false) => println!("工作区状态: clean"),
        None => println!("工作区状态: unknown（构建时无法运行 git status）"),
    }
    println!("Rust Windows 便携式 CLI");
    println!("雷电模拟器官网: {}", LDPLAYER_URL);
    println!("智汇福大官网: {}", ZHFD_URL);
    println!("配置/日志根目录: EXE 所在目录");
    println!("支持 Profile: {}", profile::PROFILES.len());
    println!("LDPlayer 当前预设: {}", profile::LDPLAYER_PRESETS.len());
    for supported in profile::PROFILES {
        println!(
            "  {} {}x{}@{} {} calibrated={} {}",
            supported.id,
            supported.width,
            supported.height,
            supported.density_dpi,
            supported.orientation.as_str(),
            supported.calibrated,
            if profile::is_ldplayer_preset(supported.width, supported.height, supported.density_dpi)
            {
                "preset"
            } else {
                "legacy"
            }
        );
    }
    println!("视觉依赖: image + 手写 HSV/像素统计/行结构");
    println!("设备依赖: ADB + LDPlayer ldconsole.exe");
    println!("GUI: eframe/egui + rfd");
}

fn config_path_command(explicit: Option<&Path>) -> Result<ExitCode> {
    let (path, origin, exists) = ConfigStore::resolve_path(explicit)?;
    println!("path={}", path.display());
    if let Some(root) = path.parent() {
        println!("root={}", root.display());
    }
    println!("origin={}", origin.as_str());
    println!("exists={}", exists);
    Ok(ExitCode::Ok)
}

fn config_command(store: &ConfigStore, command: ConfigRequest) -> Result<ExitCode> {
    match command {
        ConfigRequest::Path => {
            println!("path={}", store.path.display());
            println!("root={}", store.root.display());
            println!("origin={}", store.origin.as_str());
            println!("created_default={}", store.created_default);
        }
        ConfigRequest::Show => {
            // Redacted by default: this output is commonly pasted into issues.
            // Use `config open` (or the config file itself) when the operator
            // needs the raw values.
            println!(
                "配置文件: {}\n{}",
                store.path.display(),
                toml::to_string_pretty(&store.config.redacted_for_display())?
            );
        }
        ConfigRequest::Open => {
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

fn profile_list() -> Result<ExitCode> {
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
    Ok(ExitCode::Ok)
}

fn profile_command(store: &ConfigStore, command: ProfileCommand) -> Result<ExitCode> {
    match command {
        ProfileCommand::List => return profile_list(),
        ProfileCommand::Detect { target } => {
            let target: InstanceTargetArgs = target.into();
            let (_, device, info) = match runtime_with_target(store, false, Some(&target)) {
                Ok(value) => value,
                Err(error) => return Ok(reported_device_failure(error)),
            };
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
            let target: InstanceTargetArgs = target.into();
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
    let (_, device, info) = match runtime_with_target(store, false, Some(instance_target)) {
        Ok(value) => value,
        Err(error) => return Ok(reported_device_failure(error)),
    };
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

fn app_command(store: &ConfigStore, command: AppCommand) -> Result<ExitCode> {
    match command {
        AppCommand::Install {
            apk,
            confirm,
            target,
        } => {
            // clap already requires --confirm; keep a runtime guard so a future
            // refactor cannot bypass the acknowledgement.
            if !confirm {
                bail!("安装 APK 会写入配置并可能重启实例，请指定 --confirm");
            }
            println!("安装 APK: {}", apk.display());
            let result = match install_apk(store, &apk, &target.into()) {
                Ok(value) => value,
                Err(error) => return Ok(reported_device_failure(error)),
            };
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
    config_path: Option<&Path>,
) -> String {
    let mut task_run = format!("\"{}\" {}", exe.display(), mode);
    // The scheduled task launches a fresh process, so an explicit `--config`
    // must be pinned into the command line; otherwise the task would silently
    // run against whatever the portable/LocalAppData probe resolves to.
    if let Some(path) = config_path {
        task_run.push_str(&format!(" --config \"{}\"", path.display()));
    }
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

fn task_command(command: TaskRequest, config_path: Option<&Path>) -> Result<ExitCode> {
    match command {
        TaskRequest::Create {
            name,
            dry_run,
            live,
            confirm,
            all_instances,
            target,
        } => {
            // clap already enforces these; the runtime guards stay so a
            // programmatic caller cannot skip them.
            if live && !confirm {
                bail!("正式计划任务必须同时指定 --live --confirm");
            }
            if live && dry_run {
                bail!("--dry-run 与 --live 不能同时用于计划任务");
            }
            let current_exe = std::env::current_exe().context("无法取得 EXE 路径")?;
            let exe = cli_executable_path(&current_exe);
            let mode = if live {
                "run --live --confirm"
            } else {
                "run --dry-run"
            };
            // Load first so the task pins the *resolved absolute* config path.
            // A relative `--config` would otherwise be stored verbatim and
            // resolved against the Task Scheduler's own working directory
            // (System32), where the loader could even create a default config.
            let store = load_store_for_cli(config_path)?;
            let task_run = build_task_command_line(
                &exe,
                mode,
                all_instances,
                &target,
                Some(store.path.as_path()),
            );
            // `schtasks /TR` stores its value verbatim as the action's command
            // line. The value already quotes the executable, so wrapping the
            // whole thing in a second pair of quotes makes the Task Scheduler
            // parse the action as a program named `""` and fail to launch it
            // with 2147942487 (E_INVALIDARG). Pass it through unwrapped.
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
            let mode_label = if live { "live" } else { "dry-run" };
            println!(
                "已创建计划任务 {}（模式: {}；时间: 每天 {}）: {}",
                name, mode_label, start, task_run
            );
        }
        TaskRequest::Delete { name, confirm } => {
            if !confirm {
                bail!("删除计划任务不可恢复，请指定 --confirm");
            }
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
        TaskRequest::Query { name } => {
            let output = process::command("schtasks.exe")
                .args(["/Query", "/TN", &name, "/FO", "LIST", "/V"])
                .output()
                .context("无法执行 schtasks.exe")?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if output.status.success() {
                println!("{}", stdout);
            } else {
                // "Task does not exist" is the common case and is not an
                // execution failure, so say which one happened instead of only
                // returning a bare non-zero exit code.
                println!(
                    "计划任务不存在或无法查询: {} (schtasks 退出码 {:?})",
                    name,
                    output.status.code()
                );
                let detail = stderr.trim();
                if !detail.is_empty() {
                    println!("{}", detail);
                }
                return Ok(ExitCode::BusinessFailure);
            }
        }
    }
    Ok(ExitCode::Ok)
}

pub(crate) fn dump_ui_hierarchy(
    store: &ConfigStore,
    target: &InstanceTargetArgs,
) -> Result<String> {
    let (_, device, _) = runtime_with_target(store, false, Some(target))?;
    device.dump_hierarchy()
}

fn ui_command(store: &ConfigStore, command: UiCommand) -> Result<ExitCode> {
    match command {
        UiCommand::Dump { target } => match dump_ui_hierarchy(store, &target.into()) {
            Ok(xml) => println!("{}", xml),
            Err(error) => return Ok(reported_device_failure(error)),
        },
    }
    Ok(ExitCode::Ok)
}

/// Build a fresh run result for one execution attempt.
///
/// `status` starts as `running` and is converged exactly once by
/// [`finalize_run`]; `blocked` is the only status the batch layer sets itself.
fn new_run_result(mode: Mode, log: &RunLog, target: &InstanceTargetArgs) -> RunResult {
    RunResult {
        schema_version: 3,
        run_id: log_id(log),
        started_at: chrono::Local::now().to_rfc3339(),
        finished_at: None,
        mode: mode.as_str().into(),
        status: Some(RunStatus::Running.as_str().into()),
        blocked_by: None,
        error_code: None,
        instance_index: target.instance_index,
        instance_name: None,
        serial: target.serial.clone().unwrap_or_default(),
        profile_id: None,
        screen_width: None,
        screen_height: None,
        density_dpi: None,
        state_history: vec![],
        clicked: false,
        success: false,
        dry_run_ready: false,
        field_work_ready: false,
        auto_login_attempted: None,
        auto_login_ready: None,
        auto_login_succeeded: None,
        auto_login_method: None,
        auto_login_account_configured: None,
        auto_login_account_matched: None,
        auto_login_tap_count: None,
        login_only: None,
        page_route: None,
        checkin_mode: None,
        checkin_state: None,
        success_date: None,
        error_stage: None,
        recovery_action: None,
        artifact_paths: Vec::new(),
        page_refresh_attempted: Some(false),
        page_refresh_confirmed: Some(false),
        route: None,
        page_loading: None,
        button_state: None,
        location_state: None,
        foreground_package: None,
        hierarchy_status: None,
        overlay_package: None,
        app_version: Some(VERSION.into()),
        git_commit: Some(GIT_COMMIT.into()),
        git_dirty: git_dirty(),
        build_id: Some(build_id()),
        error: None,
    }
}

fn log_id(log: &RunLog) -> String {
    log.dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn run(store: &ConfigStore, request: RunRequest, source: RunSource) -> Result<ExitCode> {
    let policy = request.resolve(Mode::from_config(&store.config.runtime.mode)?, source);
    if policy.all_instances {
        return run_all_instances(store, &policy);
    }
    let json = policy.json;
    let (code, result) = run_single(store, &policy)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        print_run_summary(&result);
    }
    Ok(code)
}

/// Human-readable outcome for one run.
///
/// The structured contract stays in `result.json`; this is only the console
/// view, so a first-time user sees the status, the reason, and the next action
/// without reading a long JSON document.
fn print_run_summary(result: &RunResult) {
    let status = if result.login_only == Some(true) {
        match (result.auto_login_succeeded, result.auto_login_ready) {
            (Some(true), _) => "login succeeded（已确认离开登录页）",
            (_, Some(true)) => "login ready（前置条件满足，未点击）",
            _ if result.error_code.is_none() => "login: already logged in",
            _ => "login failed",
        }
    } else if result.success {
        "success（已确认签到成功）"
    } else if result.dry_run_ready {
        "dry-run ready（观察到可签到，未点击）"
    } else if result.field_work_ready {
        "field-work ready（仅观察，未点击）"
    } else if result.blocked_by.is_some() {
        "blocked"
    } else if result.error_code.is_some() {
        "failed"
    } else {
        "running"
    };
    println!("状态: {status}（{}）", result.mode);
    println!(
        "页面: {} / {} / {}",
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
    );
    println!(
        "点击: {}；登录点击: {}",
        if result.clicked { "是" } else { "否" },
        result.auto_login_tap_count.unwrap_or(0)
    );
    if let Some(code) = &result.error_code {
        println!("错误码: {code}");
    }
    if let Some(recovery) = &result.recovery_action {
        println!("下一步: {recovery}");
    }
    if let Some(path) = result.artifact_paths.first() {
        println!("结果文件: {path}");
    }
    println!("完整 JSON: 重新运行并加 --json，或读取上面的 result.json");
}

/// Execute one instance run and return both the exit code and the converged
/// result, so the batch layer can reference the real per-instance `result.json`
/// instead of re-deriving an identity for it.
fn run_single(store: &ConfigStore, policy: &RunPolicy) -> Result<(ExitCode, RunResult)> {
    let mode = policy.mode;
    let log = if policy.json {
        RunLog::new(&store.root)?.quiet()
    } else {
        RunLog::new(&store.root)?
    };
    let mut result = new_run_result(mode, &log, &policy.target);
    seed_result_identity(store, &policy.target, &mut result);
    // Provenance travels with the run itself, not only with the final result, so
    // a run can be attributed to the exact binary even when it never finishes
    // (FR-014). `git_dirty` is emitted separately because a dirty build is not
    // the commit it names.
    log.event(
        "start",
        serde_json::json!({
            "mode": mode.as_str(),
            "source": policy.source.as_str(),
            "app_version": VERSION,
            "git_commit": GIT_COMMIT,
            "git_dirty": git_dirty(),
            "build_id": build_id(),
        }),
    )?;
    if mode == Mode::Live && !policy.explicit_confirmation {
        set_run_failure(
            &mut result,
            "live_confirmation_required",
            "click_authorization",
            "正式模式需要显式 --confirm;未执行签到点击。",
        );
        finalize_run(&log, &mut result)?;
        return Ok((ExitCode::BusinessFailure, result));
    }

    let execution = run_inner(store, policy, &log, &mut result);
    let code = match execution {
        Ok(code) => code,
        Err(error) => {
            // Propagated I/O errors still converge through the single failure
            // entry, keeping the most specific stage already recorded and
            // deriving a stable machine code from it.
            let stage = result
                .error_stage
                .clone()
                .unwrap_or_else(|| "runtime".to_string());
            let (code_label, recovery) = propagated_failure(&stage);
            set_run_failure(&mut result, code_label, &stage, recovery);
            result.error = Some(format!("{:#}", error));
            eprintln!("运行失败: {:#}", error);
            ExitCode::DeviceFailure
        }
    };

    finalize_run(&log, &mut result)?;
    Ok((code, result))
}

/// Single convergence point for a run result.
///
/// Every terminal status is derived here from the accumulated evidence, so a
/// run can never be persisted while still claiming `running`, and stable
/// machine errors always reach `error_code` (frozen decisions D-05/D-07).
fn finalize_run(log: &RunLog, result: &mut RunResult) -> Result<()> {
    result.finished_at = Some(chrono::Local::now().to_rfc3339());
    if result.status.as_deref() != Some(RunStatus::Blocked.as_str()) {
        // A login-only action succeeds when it either completed the tap, only
        // confirmed the login preconditions (probe-only), or found the App
        // already past the login page. Any refusal sets `error_code`, so the
        // absence of one is the signal; `clicked` stays check-in-only.
        let login_only_ok = result.login_only == Some(true)
            && result.error_code.is_none()
            && result.auto_login_succeeded != Some(false);
        let status =
            if result.success || result.dry_run_ready || result.field_work_ready || login_only_ok {
                RunStatus::Success
            } else {
                RunStatus::Failed
            };
        result.status = Some(status.as_str().into());
    }
    if result.error_code.is_none() {
        result.error_code = result.error.clone();
    }
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

/// Two-phase batch orchestration (frozen decisions D-05/D-07, requirement
/// FR-015).
///
/// Phase 1 runs a shared dependency preflight. If it fails, no device is
/// started: every planned instance is written as an individual `blocked`
/// result and a run-level `summary.json` records the shared blocker. Phase 2
/// runs each instance independently, continues past instance-specific
/// failures, and writes the aggregate summary.
fn run_all_instances(store: &ConfigStore, policy: &RunPolicy) -> Result<ExitCode> {
    if policy.target.instance_index.is_some() || policy.target.serial.is_some() {
        bail!("--all-instances 不能与 --instance-index 或 --serial 同时使用");
    }
    let configured = store.config.emulator.effective_instances();
    if configured.is_empty() {
        bail!("没有启用的 LDPlayer 实例");
    }
    let mode = policy.mode;
    // The batch identity owns the summary directory; every per-instance result
    // stays in its own log directory and is referenced from the summary.
    let batch_id = logger::new_batch_id();
    let mut entries = Vec::new();

    if let Err(error) = shared_preflight(store) {
        let error_text = format!("{error:#}");
        println!("公共依赖 preflight 失败: {error_text}");
        // No device is started: each planned instance gets its own blocked
        // result so the instance identity is never lost to the shared blocker.
        for instance in &configured {
            let log = RunLog::new(&store.root)?;
            let log = if policy.json { log.quiet() } else { log };
            let mut result = blocked_instance_result(mode, &log, instance, &error_text);
            log.event(
                "blocked",
                serde_json::json!({"blocked_by":"shared_preflight","error":error_text}),
            )?;
            finalize_run(&log, &mut result)?;
            entries.push(instance_summary_entry(&result, RunStatus::Blocked, None));
        }
        let summary = serde_json::json!({
            "schema_version": 3,
            "app_version": VERSION,
            "git_commit": GIT_COMMIT,
            "git_dirty": git_dirty(),
            "build_id": build_id(),
            "run_id": batch_id,
            "mode": mode.as_str(),
            "status": RunStatus::Blocked.as_str(),
            "blocked_by": "shared_preflight",
            "error_code": "shared_preflight_failed",
            "error": error_text,
            "instances": entries,
        });
        let path = logger::write_batch_summary(&store.root, &batch_id, &summary)?;
        println!("批量运行被公共依赖阻断，摘要:{}", path.display());
        return Ok(ExitCode::DeviceFailure);
    }

    let mut overall = ExitCode::Ok;
    for configured_instance in &configured {
        let mut per_instance = policy.clone();
        per_instance.all_instances = false;
        per_instance.target = InstanceTargetArgs {
            instance_index: Some(configured_instance.instance_index),
            serial: (!configured_instance.serial.trim().is_empty())
                .then_some(configured_instance.serial.clone()),
        };
        println!(
            "=== 运行实例 {} ({}) serial={} ===",
            configured_instance.instance_index,
            display_instance_name(configured_instance),
            per_instance
                .target
                .serial
                .as_deref()
                .unwrap_or("按 index 推导")
        );
        // An instance-specific failure is isolated: the batch continues with the
        // remaining instances and the aggregate exit code stays non-zero. Even a
        // failure to *write* one instance's result must not abort the batch, or
        // the remaining instances would never run and no summary would exist.
        match run_single(store, &per_instance) {
            Ok((code, result)) => {
                if policy.json {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    print_run_summary(&result);
                }
                if code as i32 > overall as i32 {
                    overall = code;
                }
                let status = if code == ExitCode::Ok {
                    RunStatus::Success
                } else {
                    RunStatus::Failed
                };
                entries.push(instance_summary_entry(&result, status, Some(code as i32)));
            }
            Err(error) => {
                let error_text = format!("{error:#}");
                eprintln!(
                    "实例 {} 结果写入失败: {error_text}",
                    configured_instance.instance_index
                );
                if overall == ExitCode::Ok {
                    overall = ExitCode::DeviceFailure;
                }
                entries.push(serde_json::json!({
                    "index": configured_instance.instance_index,
                    "name": display_instance_name(configured_instance),
                    "serial": resolved_serial_for(configured_instance),
                    "status": RunStatus::Failed.as_str(),
                    "blocked_by": serde_json::Value::Null,
                    "error_code": "result_write_failed",
                    "run_id": serde_json::Value::Null,
                    "result_path": serde_json::Value::Null,
                    "exit_code": ExitCode::DeviceFailure as i32,
                    "error": error_text,
                }));
            }
        }
    }
    let summary = serde_json::json!({
        "schema_version": 3,
        "app_version": VERSION,
        "git_commit": GIT_COMMIT,
        "git_dirty": git_dirty(),
        "build_id": build_id(),
        "run_id": batch_id,
        "mode": mode.as_str(),
        "status": if overall == ExitCode::Ok {
            RunStatus::Success.as_str()
        } else {
            RunStatus::Failed.as_str()
        },
        "blocked_by": serde_json::Value::Null,
        "error_code": serde_json::Value::Null,
        "instances": entries,
    });
    let path = logger::write_batch_summary(&store.root, &batch_id, &summary)?;
    println!("批量运行摘要:{}", path.display());
    Ok(overall)
}

/// Result written for an instance that was planned but never started because a
/// shared dependency preflight failed (frozen decision D-05).
///
/// The instance identity is preserved in full: a shared blocker must not erase
/// which instances the batch intended to run.
fn blocked_instance_result(
    mode: Mode,
    log: &RunLog,
    instance: &config::EmulatorInstanceConfig,
    error_text: &str,
) -> RunResult {
    let target = InstanceTargetArgs {
        instance_index: Some(instance.instance_index),
        serial: (!instance.serial.trim().is_empty()).then_some(instance.serial.clone()),
    };
    let mut result = new_run_result(mode, log, &target);
    result.instance_index = Some(instance.instance_index);
    result.instance_name = Some(display_instance_name(instance));
    result.serial = resolved_serial_for(instance);
    result.status = Some(RunStatus::Blocked.as_str().into());
    result.blocked_by = Some("shared_preflight".into());
    result.error_code = Some("shared_preflight_failed".into());
    result.error = Some(error_text.to_string());
    result.error_stage = Some("shared_preflight".into());
    result.recovery_action = Some(
        "公共依赖(ldconsole.exe / adb.exe)不可用；请修复依赖后重新运行，本次没有启动任何设备。"
            .into(),
    );
    result
}

/// Dependencies shared by every planned instance. A failure here is not a
/// per-device fault and must not be reported as N independent device errors.
fn shared_preflight(store: &ConfigStore) -> Result<()> {
    let ld = ldplayer::LdPlayer::discover(
        &configured_path(store, &store.config.emulator.ldconsole_path),
        &store.root,
    )?;
    let instances = ld.instances()?;
    if instances.is_empty() {
        bail!("ldconsole list2 未发现任何实例");
    }
    let adb_path = adb::find_adb_for_ldplayer(
        &store.config.emulator.adb_path,
        Some(&ld.executable),
        Some(&store.root),
    )?;
    let output = process::command(&adb_path)
        .arg("version")
        .output()
        .with_context(|| format!("无法执行 ADB: {}", adb_path.display()))?;
    if !output.status.success() {
        bail!(
            "ADB version 失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn display_instance_name(instance: &config::EmulatorInstanceConfig) -> String {
    if instance.name.trim().is_empty() {
        format!("实例 {}", instance.instance_index)
    } else {
        instance.name.clone()
    }
}

fn resolved_serial_for(instance: &config::EmulatorInstanceConfig) -> String {
    if instance.serial.trim().is_empty() {
        ldplayer::LdPlayer::tcp_serial_for_index(instance.instance_index).unwrap_or_default()
    } else {
        instance.serial.clone()
    }
}

/// One machine-readable entry per planned instance in the batch summary. The
/// instance identity (`index`/`name`/`serial`) survives even when the run is
/// blocked, so a shared blocker never erases which instances were planned.
fn instance_summary_entry(
    result: &RunResult,
    status: RunStatus,
    exit_code: Option<i32>,
) -> serde_json::Value {
    serde_json::json!({
        "index": result.instance_index,
        "name": result.instance_name,
        "serial": result.serial,
        "status": status.as_str(),
        "blocked_by": result.blocked_by,
        "error_code": result.error_code,
        "run_id": result.run_id,
        "result_path": result.artifact_paths.first(),
        "exit_code": exit_code,
    })
}

fn login_command(store: &ConfigStore, args: LoginArgs) -> Result<ExitCode> {
    // No `--confirm-login` means "probe only": the same gates run, but the
    // policy stays in DryRun and nothing is tapped.
    let mode = if args.confirm_login {
        Mode::Live
    } else {
        Mode::DryRun
    };
    let log = if args.json {
        RunLog::new(&store.root)?.quiet()
    } else {
        RunLog::new(&store.root)?
    };
    let target: InstanceTargetArgs = args.target.clone().into();
    let mut result = new_run_result(mode, &log, &target);
    result.login_only = Some(true);
    seed_result_identity(store, &target, &mut result);
    log.event(
        "start",
        serde_json::json!({
            "mode": mode.as_str(),
            "action": "login_only",
            "app_version": VERSION,
            "git_commit": GIT_COMMIT,
            "git_dirty": git_dirty(),
            "build_id": build_id(),
        }),
    )?;

    let code = login_inner(store, &args, mode, &log, &mut result)?;
    finalize_run(&log, &mut result)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        print_run_summary(&result);
    }
    Ok(code)
}

fn login_inner(
    store: &ConfigStore,
    args: &LoginArgs,
    mode: Mode,
    log: &RunLog,
    result: &mut RunResult,
) -> Result<ExitCode> {
    // With `--json`, stdout must stay a single machine-readable document; the
    // human-facing notes below go to stderr instead.
    let note = |text: &str| {
        if args.json {
            eprintln!("{text}");
        } else {
            println!("{text}");
        }
    };
    result.error_stage = Some("device_setup".into());
    let target: InstanceTargetArgs = args.target.clone().into();
    let (_ld, device, info) =
        match runtime_with_target_recording(store, true, Some(&target), Some(result)) {
            Ok(value) => value,
            Err(error) => return Ok(reported_device_failure(error)),
        };

    // Profile detection is informational here: login taps use hierarchy bounds,
    // so an unknown or uncalibrated profile must not block session setup.
    result.error_stage = Some("profile_detection".into());
    result.screen_width = Some(info.width);
    result.screen_height = Some(info.height);
    result.density_dpi = Some(info.density_dpi);
    if let Some(profile) = profile::find_profile(info.width, info.height, info.density_dpi) {
        result.profile_id = Some(profile.id.to_string());
    } else {
        log.event(
            "profile_unknown",
            serde_json::json!({"width":info.width,"height":info.height,"dpi":info.density_dpi}),
        )?;
    }

    result.error_stage = Some("app_launch".into());
    launch_package_if_needed(&device, store, log)?;
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
            "请在模拟器中手动打开目标 App 后重新运行。",
        );
        return Ok(ExitCode::Timeout);
    }
    thread::sleep(Duration::from_millis(800));
    log_foreground_snapshot(&device, log, "after_wait_for_package", result)?;

    result.error_stage = Some("auto_login".into());
    let attempt = auto_login::login_only(
        &device,
        log,
        result,
        store,
        mode,
        &store.config.app.package_name,
    )?;
    let code = match attempt {
        auto_login::LoginAttempt::Disabled => {
            set_run_failure(
                result,
                "auto_login_disabled",
                "auto_login",
                "自动登录未启用；请先设置 runtime.allow_auto_login = true。",
            );
            ExitCode::BusinessFailure
        }
        auto_login::LoginAttempt::AlreadyLoggedIn => {
            // Already past the login page: success with no tap.
            note("当前已登录（页面不是登录页）；未执行任何点击。");
            ExitCode::Ok
        }
        auto_login::LoginAttempt::NotLoginPage => {
            set_run_failure(
                result,
                "not_login_page",
                "auto_login",
                "当前不是登录页（可能已登录）；无需执行 login。",
            );
            ExitCode::BusinessFailure
        }
        auto_login::LoginAttempt::Ready => {
            note("登录页前置条件满足；未传 --confirm-login，因此没有点击。");
            ExitCode::Ok
        }
        auto_login::LoginAttempt::Succeeded => ExitCode::Ok,
        auto_login::LoginAttempt::Refused(code) => auto_login::exit_code_for(code),
    };
    Ok(code)
}

fn run_inner(
    store: &ConfigStore,
    policy: &RunPolicy,
    log: &RunLog,
    result: &mut RunResult,
) -> Result<ExitCode> {
    run_inner_with_device(
        store,
        policy,
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

/// Keep the typed `page_route` and the machine `route` string in lockstep.
///
/// Both fields are part of the v0.3 result contract; letting them drift is what
/// previously left `route` permanently `null` while `page_route` carried the
/// real decision.
fn set_page_route(result: &mut RunResult, route: PageRoute) {
    result.page_route = Some(route);
    result.route = Some(route.as_str().to_string());
}

fn set_checkin_mode(result: &mut RunResult, mode: CheckinMode) {
    result.checkin_mode = Some(mode);
}

/// Stable machine code and recovery advice for an error that propagated out of
/// `run_inner`, derived from the stage that was current when it happened.
///
/// Without this the whole class of I/O failures collapsed into one
/// `runtime_error` code and lost the stage that produced it.
fn propagated_failure(stage: &str) -> (&'static str, &'static str) {
    match stage {
        "device_setup" => (
            "device_setup_failed",
            "无法准备设备(模拟器/ADB)；请检查实例是否可启动、serial 是否正确，然后重试。",
        ),
        "app_launch" | "app_foreground" => (
            "app_start_failed",
            "目标 App 未能进入前台；请在模拟器中手动打开并登录后重试。",
        ),
        "visual_observation" | "page_observation" => (
            "observation_failed",
            "截图或页面层级读取失败；请检查设备状态后重试。",
        ),
        _ => ("runtime_error", "请查看诊断报告和最新运行截图后重试。"),
    }
}

/// Hard upper bound for the locating wait, in seconds.
///
/// Frozen decision D-09 fixes the cap at 120 s; the configuration may only make
/// the wait shorter, never longer, so a mistyped `location_timeout_seconds`
/// cannot turn the bounded wait into an unbounded one.
fn location_wait_limit(store: &ConfigStore) -> u64 {
    const HARD_CAP_SECONDS: u64 = 120;
    store
        .config
        .runtime
        .location_timeout_seconds
        .clamp(1, HARD_CAP_SECONDS)
}

/// The single failure entry point.
///
/// Every terminal failure goes through here so that the stable machine code,
/// the user-readable error, the failure stage and the recovery suggestion can
/// never drift apart (`status` is then converged by [`finalize_run`]).
/// Map the visual button state onto the machine `button_state` field. This is
/// deliberately separate from `checkin_state`: "the button is not ready" is not
/// the same fact as "the page is unknown" (requirement FR-016).
fn button_state_label(state: vision::ButtonState) -> &'static str {
    match state {
        vision::ButtonState::Gray => "gray",
        vision::ButtonState::Locating => "locating",
        vision::ButtonState::Ready => "ready",
        vision::ButtonState::Success => "success",
        vision::ButtonState::Unknown => "unknown",
    }
}

/// Map the visual button state onto the machine `location_state` field.
fn location_state_label(state: vision::ButtonState) -> &'static str {
    match state {
        vision::ButtonState::Locating => "locating",
        vision::ButtonState::Ready => "ready",
        vision::ButtonState::Success => "success",
        vision::ButtonState::Gray => "unknown",
        vision::ButtonState::Unknown => "unknown",
    }
}

fn describe_route_failure(route: PageRoute) -> (&'static str, &'static str) {
    match route {
        PageRoute::Login => (
            "login_required",
            "请在模拟器中手动完成登录；若已显式启用 runtime.allow_auto_login，程序会先尝试一次一键登录。",
        ),
        PageRoute::RouteCandidateUnconfirmed => (
            "route_candidate_unconfirmed",
            "页面候选缺少可验证的层级证据；请手动确认页面后重试。",
        ),
        PageRoute::RouteEvidenceConflict => (
            "route_evidence_conflict",
            "页面截图和层级证据不一致；请手动确认页面后重试。",
        ),
        PageRoute::StalePageRefreshFailed => (
            "stale_page_refresh_failed",
            "启动时停留在旧的签到页，返回后未回到服务页；请手动返回首页后重新运行。",
        ),
        PageRoute::StalePageRefreshUnconfirmed => (
            "stale_page_refresh_unconfirmed",
            "启动时停留在旧的签到页，返回后页面层级无法读取；请手动返回首页后重新运行。",
        ),
        PageRoute::AppRestartDialog => (
            "app_restart_dialog",
            "检测到 App 的“是否重启App / 当前 App 可能被界面劫持”安全弹窗；程序未点击。请在模拟器中点击“取消”或“重启App”，并检查是否有悬浮窗/录屏/辅助功能后重新运行。",
        ),
        PageRoute::AppLeftForeground => (
            "target_app_not_foreground",
            "启动时停留在旧的签到页，返回后目标 App 已离开前台（可能退到了桌面）；程序未执行点击。请手动把 App 的普通签到页切回前台后重新运行。",
        ),
        _ => (
            route.error_code().unwrap_or("navigation_not_confirmed"),
            "请手动进入“晚点名签到”页面后重新运行;未执行签到点击。",
        ),
    }
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

/// Extract the success date from a check-in page hierarchy.
///
/// The observed success page splits a label from its value across sibling
/// nodes, so a date is not always in the same node as the text that gives it
/// meaning:
///
/// ```text
/// <node content-desc="date" />            <node text="2026-10-17 星期六" />
/// <node text="已签到：" />                 <node text="2026-10-17 22:37:10" />
/// ```
///
/// A date therefore counts as contextual when it is either
/// * in a node that itself says `签到成功` / `已签到`, or starts with `日期`, or
/// * the value of the node directly following an explicit date label
///   (`content-desc="date"`, or text `已签到` / `已签到：`).
///
/// A generic `签到成功` banner deliberately does **not** anchor the next node:
/// otherwise an unrelated date such as `上次更新 2026-10-11` would inherit its
/// context and be reported as the check-in date.
///
/// The result stays `None` unless exactly one contextual date exists and every
/// date on the page agrees with it, so conflicting evidence fails closed.
///
/// Nodes that declare a `package` other than `target_package` are ignored
/// entirely: a status-bar clock, a foreign overlay, or a hidden system node
/// must never supply (or veto) the check-in date. Nodes without a `package`
/// attribute are kept, matching [`PageView::parse`].
fn extract_hierarchy_date(xml: &str, target_package: &str) -> Option<chrono::NaiveDate> {
    let mut all_dates = Vec::new();
    let mut contextual_dates = Vec::new();
    // Labels of the previous sibling node, used to anchor a split value.
    let mut previous_labels: Vec<String> = Vec::new();

    for chunk in xml.split("<node").skip(1) {
        let Some(tag_end) = tag_body_end(chunk, 0) else {
            continue;
        };
        let body = &chunk[..tag_end];
        if let Some(package) = attribute_value(body, "package")
            && !package.is_empty()
            && package != target_package
        {
            // Foreign window: contributes neither dates nor an anchor.
            continue;
        }
        let text = attribute_value(body, "text").unwrap_or("");
        let content_desc = attribute_value(body, "content-desc").unwrap_or("");
        let labels: Vec<String> = [text, content_desc]
            .into_iter()
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect();

        let anchored_by_previous = previous_labels
            .iter()
            .any(|label| is_date_or_status_label(label));
        let mut node_dates = Vec::new();
        for label in &labels {
            for date in extract_dates_from_text(label) {
                if !node_dates.contains(&date) {
                    node_dates.push(date);
                }
                if !all_dates.contains(&date) {
                    all_dates.push(date);
                }
            }
        }
        let has_checkin_context = labels
            .iter()
            .any(|label| label.contains("签到成功") || label.contains("已签到"));
        let has_explicit_date_label = labels
            .iter()
            .any(|label| label.trim_start().starts_with("日期"));
        if has_checkin_context || has_explicit_date_label || anchored_by_previous {
            for date in node_dates {
                if !contextual_dates.contains(&date) {
                    contextual_dates.push(date);
                }
            }
        }

        // Only label-bearing nodes replace the anchor; intermediate layout
        // containers with no text must not break a label/value pair.
        if !labels.is_empty() {
            previous_labels = labels;
        }
    }

    if contextual_dates.len() != 1 || all_dates.iter().any(|date| *date != contextual_dates[0]) {
        return None;
    }
    contextual_dates.first().copied()
}

/// Whether a node's own label introduces the date that follows it.
///
/// Kept deliberately narrow: `签到成功` is a result banner that can appear next
/// to an unrelated date, so it is not a label for the following node.
fn is_date_or_status_label(label: &str) -> bool {
    let trimmed = label.trim();
    if trimmed.eq_ignore_ascii_case("date") {
        return true;
    }
    let normalized = trimmed.trim_end_matches([':', '：']).trim();
    normalized == "已签到"
}
fn run_inner_with_device<F>(
    store: &ConfigStore,
    policy: &RunPolicy,
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
    let mode = policy.mode;
    result.error_stage = Some("device_setup".into());
    let (ld, device, info) = runtime_provider(store, true, &policy.target, result)?;
    result.serial = device.serial.clone();
    let selected = resolve_instance_target(&ld, store, Some(&policy.target))?;
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
    launch_package_if_needed(&device, store, log)?;
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
    log_foreground_snapshot(&device, log, "after_wait_for_package", result)?;

    // Optional one-click login. This only runs when `runtime.allow_auto_login`
    // is enabled; a disabled feature costs no extra hierarchy dump, and any
    // refusal stops the run with a stable `auto_login_*` code before the
    // check-in navigation starts.
    result.error_stage = Some("auto_login".into());
    if let Some(code) = auto_login::auto_login_if_required(
        &device,
        log,
        result,
        mode,
        store,
        &store.config.app.package_name,
    )? {
        return Ok(code);
    }

    result.error_stage = Some("navigation".into());
    let navigation = locate_or_navigate(
        &device,
        profile,
        log,
        result,
        &store.config.app.package_name,
    )?;
    result.overlay_package = navigation.overlay_package.clone();
    set_page_route(result, navigation.route);
    set_checkin_mode(result, navigation.checkin_mode);
    result.hierarchy_status = navigation.hierarchy_status.map(str::to_string);
    result.page_refresh_attempted = Some(navigation.page_refresh_attempted);
    result.page_refresh_confirmed = Some(navigation.page_refresh_confirmed);
    log.event(
        "page_observation",
        serde_json::json!({
            "route": navigation.route.as_str(),
            "checkin_mode": navigation.checkin_mode.as_str(),
        }),
    )?;
    if navigation.route != PageRoute::Checkin {
        let (code, recovery) = match navigation.route {
            PageRoute::RouteCandidateUnconfirmed => {
                unconfirmed_page_failure(navigation.overlay_package.as_deref())
            }
            route => {
                let (code, recovery) = describe_route_failure(route);
                (code, recovery.to_string())
            }
        };
        set_run_failure(result, code, "navigation", &recovery);
        return Ok(ExitCode::VisionFailure);
    }
    // Field work ("外勤签到") is reached from its own control and is a different
    // business action from the ordinary evening check-in. It therefore has its
    // own opt-in switch (`runtime.allow_field_work_live`) instead of being
    // folded into the ordinary-mode gate:
    //
    //   dry-run  -> always observe only, never tap;
    //   live     -> refused unless the operator opted in;
    //   live+opt -> continue into the same observe/confirm/tap loop below.
    //
    // The opt-in never widens any other gate: route must still be the check-in
    // page, the profile must be calibrated, the app must be in the foreground,
    // the configured window must be open, the control must be Ready for the
    // configured number of frames, and the pre-tap re-observation still runs.
    let allow_field_work = store.config.runtime.allow_field_work_live;
    if navigation.checkin_mode == CheckinMode::FieldWork {
        result.error_stage = Some("field_work_observation".into());
        log.event(
            "field_work_observation",
            serde_json::json!({
                "route": navigation.route.as_str(),
                "mode": navigation.checkin_mode.as_str(),
                "state": "observation_pending",
                "allow_field_work_live": allow_field_work,
                "clicked": false,
            }),
        )?;
        if mode == Mode::Live && !allow_field_work {
            result.checkin_state = Some(CheckinState::Unknown);
            set_run_failure(
                result,
                "field_work_live_unsupported",
                "click_authorization",
                "外勤签到自动点击未启用；请在 config.toml 设置 runtime.allow_field_work_live = true，或在模拟器中人工完成签到。",
            );
            return Ok(ExitCode::BusinessFailure);
        }

        if mode == Mode::DryRun {
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

        // Live with the opt-in: fall through to the shared run loop. The loop
        // re-observes the page and the foreground package before every state
        // observation and before the tap, exactly as it does for ordinary mode.
        log.event(
            "field_work_live_authorized",
            serde_json::json!({
                "route": navigation.route.as_str(),
                "mode": navigation.checkin_mode.as_str(),
                "allow_field_work_live": true,
                "clicked": false,
            }),
        )?;
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
            if started.elapsed() >= Duration::from_secs(location_wait_limit(store)) {
                // Frozen decision D-09: the bounded locating wait has a stable
                // machine code and keeps its last evidence.
                set_run_failure(
                    result,
                    "location_wait_timeout",
                    "checkin_wait",
                    "定位等待达到 120 秒上限；已保留最后一轮证据并安全停止，未执行点击。",
                );
                break;
            }
            // D-09 also stops when the configured window has less than five
            // seconds left: there is no longer a safe window to click in.
            if let Some(remaining) = domain::seconds_until_configured_window_end(
                &store.config.window.start,
                &store.config.window.end,
                &store.config.window.timezone,
            )? && remaining < 5
            {
                set_run_failure(
                    result,
                    "location_wait_timeout",
                    "checkin_wait",
                    "签到窗口剩余不足 5 秒，已停止等待并保留最后一轮证据，未执行点击。",
                );
                break;
            }
        }

        // Every round records the foreground package and a hierarchy
        // observation with its own id, so a route decision can always be tied
        // back to the same foreground read, dump status and screenshot
        // (requirements FR-011/FR-012/FR-013).
        result.error_stage = Some("page_observation".into());
        let observation_id = log.next_observation_id();
        if !require_target_foreground(
            &device,
            log,
            "state_poll",
            result,
            &store.config.app.package_name,
        )? {
            break;
        }
        let page = observe_page_route(
            &device,
            log,
            "state_poll",
            observation_id,
            &store.config.app.package_name,
        )?;
        let current_route = page.route;
        let current_mode = page.checkin_mode;
        result.overlay_package = page.foreign_package.clone();
        result.hierarchy_status = Some(page.status.into());
        set_page_route(result, current_route);
        set_checkin_mode(result, current_mode);
        // `page_loading` is its own dimension and must not be inferred from an
        // unreadable hierarchy. It stays unknown here and is only set once the
        // button observation for this round provides evidence.
        result.page_loading = None;
        if let Some(package) = page.foreign_package.as_deref() {
            let (code, recovery) = unconfirmed_page_failure(Some(package));
            set_run_failure(result, code, "page_observation", &recovery);
            break;
        }
        match current_route {
            // A dump that failed or returned no usable page identity is its own
            // diagnosis, not "the page changed".
            PageRoute::RouteCandidateUnconfirmed => {
                let (code, recovery) = unconfirmed_page_failure(page.foreign_package.as_deref());
                set_run_failure(result, code, "page_observation", &recovery);
                break;
            }
            PageRoute::RouteEvidenceConflict => {
                set_run_failure(
                    result,
                    "route_evidence_conflict",
                    "page_observation",
                    "页面截图和层级证据不一致；请人工确认页面后重试。未执行签到点击。",
                );
                break;
            }
            // Ordinary check-in is always authorized on the check-in page.
            PageRoute::Checkin if current_mode == CheckinMode::Ordinary => {}
            // Field work only reaches the loop when the operator opted in; if
            // the page flips back to FieldWork mid-run without the opt-in it
            // must stop here rather than being tapped.
            PageRoute::Checkin if current_mode == CheckinMode::FieldWork && allow_field_work => {}
            PageRoute::Checkin => {
                // Defensive fallback: observe_page_route normally classifies a
                // confirmed check-in page as Ordinary or FieldWork. If a future
                // classifier adds another mode, this remains fail-closed.
                result.checkin_state = Some(CheckinState::Unknown);
                set_run_failure(
                    result,
                    "unsupported_checkin_mode",
                    "checkin_classification",
                    if current_mode == CheckinMode::FieldWork {
                        "页面已切换到外勤签到，而其自动点击未启用；程序已停止自动点击。"
                    } else {
                        "当前签到模式不是普通晚点名签到；程序已停止自动点击。"
                    },
                );
                break;
            }
            other => {
                // Home/Services/Login on a poll round: the page is readable but
                // is no longer the ordinary check-in page.
                let code = other.error_code().unwrap_or("page_context_changed");
                set_run_failure(
                    result,
                    code,
                    "page_observation",
                    "请返回普通晚点名签到页面并重新运行；程序已停止自动点击。",
                );
                break;
            }
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
        result.button_state = Some(button_state_label(analysis.state).into());
        result.location_state = Some(location_state_label(analysis.state).into());
        result.checkin_state = Some(checkin_state_from_visual(analysis.state));
        // The page is observably settled once the control itself can be read.
        // While it cannot, the loading dimension stays unknown rather than
        // being asserted as "still loading".
        result.page_loading = Some(analysis.state == vision::ButtonState::Unknown);
        log.event("state", &analysis)?;

        if analysis.state == vision::ButtonState::Success {
            result.error_stage = Some("success_confirmation".into());
            // Re-read the hierarchy for the success round instead of reusing
            // the earlier round's copy; a failed read downgrades the route
            // rather than being silently treated as "no success text".
            let success_round = log.next_observation_id();
            let success_page = observe_page_route(
                &device,
                log,
                "success_confirmation",
                success_round,
                &store.config.app.package_name,
            )?;
            let success_route = success_page.route;
            // Marker checks read the scoped text; date extraction needs the raw
            // document, because the date and its label live in separate nodes.
            let success_xml = &success_page.view.scoped_text;
            let success_document = &success_page.xml;
            result.overlay_package = success_page.foreign_package.clone();
            result.hierarchy_status = Some(success_page.status.into());
            if success_route != PageRoute::Checkin {
                set_page_route(result, success_route);
                let (code, recovery) = describe_route_failure(success_route);
                set_run_failure(result, code, "success_confirmation", recovery);
                log.event(
                    "checkin_semantic_state",
                    serde_json::json!({
                        "route": success_route.as_str(),
                        "mode": current_mode.as_str(),
                        "state": CheckinState::Unknown.as_str(),
                        "reason": "success_round_route_unconfirmed",
                    }),
                )?;
                break;
            }
            if !["签到成功", "已签到"]
                .iter()
                .any(|marker| success_xml.contains(marker))
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
            match extract_hierarchy_date(success_document, &store.config.app.package_name) {
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
                if analysis.state == vision::ButtonState::Locating {
                    log.event(
                        "locating_wait",
                        serde_json::json!({
                            "elapsed_seconds": started.elapsed().as_secs(),
                            "poll_seconds": store.config.runtime.locating_poll_seconds,
                            "limit_seconds": store.config.runtime.location_timeout_seconds,
                            "stop_conditions": [
                                "ready",
                                "success_or_failure",
                                "foreground_lost",
                                "window_remaining_below_5s",
                                "limit_reached",
                            ],
                            "clicked": false,
                        }),
                    )?;
                }
                // Bounded by the locating budget and the window end, so a
                // 2-second poll can never overshoot either deadline.
                sleep_for_next_poll(store, started)?;
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

                // Live gate: re-read the page and the foreground package
                // immediately before the tap. `navigated=true` is never treated
                // as evidence that the check-in page is still on screen.
                result.error_stage = Some("click_authorization".into());
                let tap_round = log.next_observation_id();
                let tap_page = observe_page_route(
                    &device,
                    log,
                    "before_live_tap",
                    tap_round,
                    &store.config.app.package_name,
                )?;
                let confirmed_route = tap_page.route;
                let confirmed_mode = tap_page.checkin_mode;
                result.overlay_package = tap_page.foreign_package.clone();
                result.hierarchy_status = Some(tap_page.status.into());
                if confirmed_route != PageRoute::Checkin {
                    let (code, recovery) = describe_route_failure(confirmed_route);
                    set_run_failure(result, code, "click_authorization", recovery);
                    break;
                }
                // The mode that authorizes the tap must match the mode this run
                // was admitted with. Ordinary is always allowed; field work only
                // when the operator opted in; anything else fails closed.
                let mode_authorized = match confirmed_mode {
                    CheckinMode::Ordinary => true,
                    CheckinMode::FieldWork => allow_field_work,
                    CheckinMode::Unsupported => false,
                };
                if !mode_authorized {
                    // Still the check-in page, but no longer a mode this run may
                    // tap: the tap is not authorized for an unconfirmed mode, nor
                    // for field work without the explicit opt-in.
                    set_run_failure(
                        result,
                        "page_context_changed",
                        "click_authorization",
                        if confirmed_mode == CheckinMode::FieldWork {
                            "页面已切换到外勤签到，而其自动点击未启用；程序未执行点击。"
                        } else {
                            "签到模式已无法确认；程序未执行点击，请确认页面后重试。"
                        },
                    );
                    break;
                }
                let foreground_matches = require_target_foreground(
                    &device,
                    log,
                    "before_live_tap",
                    result,
                    &store.config.app.package_name,
                )?;
                if !foreground_matches {
                    break;
                }
                let confirm_bytes = device.screenshot()?;
                let confirm_image = vision::decode(&confirm_bytes)?;
                // The tap coordinate must come from THIS screenshot. Reusing the
                // earlier round's box would tap a stale position; when nothing is
                // detected, fall back to this image's profile geometry.
                let confirm_button = vision::find_colored_button(&confirm_image)
                    .unwrap_or_else(|| vision::fallback_button(&confirm_image, profile));
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
                        allow_field_work: store.config.runtime.allow_field_work_live,
                        mode,
                        explicit_confirmation: policy.explicit_confirmation,
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
        Some("location_wait_timeout" | "location_timeout" | "success_timeout")
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
    /// Operator opt-in for tapping the field-work control
    /// (`runtime.allow_field_work_live`). Carried through to
    /// `authorize_page_action` so the policy decision lives in `domain`.
    allow_field_work: bool,
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
    domain::authorize_page_action(
        request.route,
        request.checkin_mode,
        request.allow_field_work,
    )?;
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

/// Sleep until the next locating poll, bounded by both the remaining locating
/// budget and the remaining configured window (frozen decision D-09).
fn sleep_for_next_poll(store: &ConfigStore, started: Instant) -> Result<()> {
    let location_remaining =
        Duration::from_secs(location_wait_limit(store).saturating_sub(started.elapsed().as_secs()));
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

/// Launch the target App only when it is not already the foreground package.
///
/// `am start` restarts the existing activity even when the App is already on
/// screen. The App's own anti-hijack prompt can appear after such a restart
/// (observed on 2026-10-21 as "是否重启App / 当前 App 可能被界面劫持"), so a
/// no-op launch avoids one known trigger without skipping the foreground wait.
fn launch_package_if_needed(
    device: &adb::AdbDevice,
    store: &ConfigStore,
    log: &RunLog,
) -> Result<()> {
    let package = &store.config.app.package_name;
    match device.current_package() {
        Ok(current) if current == *package => {
            log.event(
                "app_launch_skipped",
                serde_json::json!({"package": package, "reason": "already_foreground"}),
            )?;
        }
        _ => device.launch_package(package, &store.config.app.activity_name)?,
    }
    Ok(())
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
    page_refresh_attempted: bool,
    page_refresh_confirmed: bool,
    /// Machine result of the hierarchy read that produced `route`
    /// (`dump_succeeded`, `dump_failed`, `dump_empty_or_truncated`,
    /// `route_markers_missing`). `None` when no observation was needed.
    hierarchy_status: Option<&'static str>,
    /// Package of a window that was on screen but does not belong to the
    /// target app, when one was seen. This is the direct evidence of a blocking
    /// popup or advertisement.
    overlay_package: Option<String>,
}

/// Machine code and recovery advice for a page that could not be identified.
///
/// A foreign window on screen and a page that simply carries no identity
/// markers need different user actions, so they get different codes.
fn unconfirmed_page_failure(overlay: Option<&str>) -> (&'static str, String) {
    match overlay {
        Some(package) => (
            "overlay_present",
            format!(
                "检测到非目标应用的窗口（{package}）遮挡了页面；请手动关闭该弹窗/广告后重新运行。程序未执行任何点击。"
            ),
        ),
        None => (
            "route_candidate_unconfirmed",
            "页面层级可读，但没有页面身份标记（可能被弹窗遮挡，或页面尚未渲染完成）；请手动确认页面后重试。"
                .to_string(),
        ),
    }
}

fn is_navigable_catalog_route(route: PageRoute) -> bool {
    matches!(route, PageRoute::Home | PageRoute::Services)
}
/// Visual evidence for the check-in control, kept separate from the page
/// identity decided by the hierarchy.
///
/// Only `Detected` may participate in an *evidence conflict*: a saturated
/// control really found in the screenshot disagreeing with the hierarchy is a
/// conflict. A profile-geometry fallback box that merely happens to contain
/// colour is a weak candidate and must never be reported as a conflict
/// (frozen decision D-08).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VisualCandidate {
    Detected(vision::ButtonState),
    GeometryOnly(vision::ButtonState),
    None,
}

impl VisualCandidate {
    fn state(self) -> vision::ButtonState {
        match self {
            Self::Detected(state) | Self::GeometryOnly(state) => state,
            Self::None => vision::ButtonState::Unknown,
        }
    }
}

fn visual_candidate(image: &image::DynamicImage, profile: &profile::Profile) -> VisualCandidate {
    match vision::find_colored_button(image) {
        Some(button) => VisualCandidate::Detected(vision::classify_button(image, button).state),
        None => {
            let fallback = vision::fallback_button(image, profile);
            let state = vision::classify_button(image, fallback).state;
            if state == vision::ButtonState::Unknown {
                VisualCandidate::None
            } else {
                VisualCandidate::GeometryOnly(state)
            }
        }
    }
}

/// How many times an unidentifiable page is re-observed before giving up.
///
/// A single unlucky dump — an overlay that dismisses itself, or a WebView that
/// has not finished rendering — used to end the whole run. Re-reading the page
/// is free of any tap, so it is retried a bounded number of times first.
const UNCONFIRMED_REOBSERVE_ATTEMPTS: u32 = 3;
const UNCONFIRMED_REOBSERVE_INTERVAL: Duration = Duration::from_secs(2);

/// Whether a page observed after the stale-page BACK refresh may still be used.
///
/// `Home`/`Services` means the refresh reached the catalogue. A page that is
/// *still* the check-in page, owned by the target app, with no foreign window
/// may also be used: some app builds swallow BACK on the check-in page (verified
/// on-device, where repeated keyevents leave the hierarchy byte-identical), and
/// the run loop re-observes the page before every decision, so continuing does
/// not weaken any gate.
fn may_continue_on_checkin_page(
    route: PageRoute,
    foreign_package_present: bool,
    app_foreground: bool,
) -> bool {
    route == PageRoute::Checkin && !foreign_package_present && app_foreground
}

/// Whether an unidentifiable page should be read again before giving up.
///
/// Pure so the bound is testable without a device: the loop must terminate, and
/// it must only ever be reached for an unconfirmed page.
fn should_reobserve(route: PageRoute, attempts: u32) -> bool {
    route == PageRoute::RouteCandidateUnconfirmed && attempts < UNCONFIRMED_REOBSERVE_ATTEMPTS
}

fn locate_or_navigate(
    device: &adb::AdbDevice,
    profile: &profile::Profile,
    log: &RunLog,
    result: &mut RunResult,
    target_package: &str,
) -> Result<NavigationResult> {
    let mut bytes = device.screenshot()?;
    let mut image = vision::decode(&bytes)?;
    // The most recent observation of the page, reused by later decisions
    // instead of re-parsing a string that may belong to an earlier round.
    let mut current: Option<PageObservation> = None;
    let mut last_status: Option<&'static str> = None;
    let mut observe = |stage: &str| -> Result<PageObservation> {
        let observation_id = log.next_observation_id();
        let observed = observe_page_route(device, log, stage, observation_id, target_package)?;
        last_status = Some(observed.status);
        Ok(observed)
    };

    log_foreground_snapshot(device, log, "navigation_start", result)?;
    let mut observation = observe("navigation_start")?;
    let mut attempts = 1u32;
    while should_reobserve(observation.route, attempts) {
        // Observing only: no tap, no swipe, no navigation while the page is
        // unconfirmed. An overlay that dismisses itself, or a slow first paint,
        // recovers here instead of failing the run.
        log.event(
            "reobserve",
            serde_json::json!({
                "stage": "navigation_start",
                "attempt": attempts + 1,
                "status": observation.status,
                "overlay_package": observation.foreign_package,
                "clicked": false,
            }),
        )?;
        thread::sleep(UNCONFIRMED_REOBSERVE_INTERVAL);
        log_foreground_snapshot(device, log, "navigation_reobserve", result)?;
        bytes = device.screenshot()?;
        image = vision::decode(&bytes)?;
        observation = observe("navigation_reobserve")?;
        attempts += 1;
    }

    let mut page_refresh_attempted = false;
    let mut page_refresh_confirmed = false;
    // Set when BACK could not leave a confirmed check-in page (some app builds
    // swallow the key). The page is then used as-is instead of failing.
    let mut continued_on_checkin_page = false;

    // A page that still cannot be identified is a *candidate*, never a
    // confirmed route: no navigation action and no tap may follow from it.
    if observation.route == PageRoute::RouteCandidateUnconfirmed {
        log.save_bytes("navigation_result.png", &bytes)?;
        return Ok(NavigationResult {
            route: observation.route,
            checkin_mode: CheckinMode::Unsupported,
            field_work_ready: false,
            page_refresh_attempted,
            page_refresh_confirmed,
            hierarchy_status: last_status,
            overlay_package: observation.foreign_package,
        });
    }
    if observation.route == PageRoute::Login {
        log.save_bytes("navigation_result.png", &bytes)?;
        return Ok(NavigationResult {
            route: observation.route,
            checkin_mode: CheckinMode::Unsupported,
            field_work_ready: false,
            page_refresh_attempted,
            page_refresh_confirmed,
            hierarchy_status: last_status,
            overlay_package: observation.foreign_package,
        });
    }

    // Any foreign root window is a blocking condition, regardless of whether
    // the target app's own nodes still expose a catalogue or check-in marker.
    // Stop before BACK, swipe, or tap: the OS may deliver those actions to the
    // foreign window rather than to the target app.
    if let Some(package) = observation.foreign_package.clone() {
        log.event(
            "navigation_blocked",
            serde_json::json!({
                "reason": "overlay_present",
                "overlay_package": package,
                "clicked": false,
            }),
        )?;
        log.save_bytes("navigation_result.png", &bytes)?;
        return Ok(NavigationResult {
            route: PageRoute::RouteCandidateUnconfirmed,
            checkin_mode: CheckinMode::Unsupported,
            field_work_ready: false,
            page_refresh_attempted,
            page_refresh_confirmed,
            hierarchy_status: last_status,
            overlay_package: Some(package),
        });
    }

    // Starting already on a check-in page means a previous run left the App
    // there; go back to the catalogue so the normal navigation path is
    // exercised and the page state is observed fresh.
    //
    // The BACK keyevent is only issued while the check-in page is confirmed
    // *from the target app's own nodes* and no other window is on screen: with
    // a popup present there is no way to tell which window BACK would act on.
    let on_checkin_page = observation.route == PageRoute::Checkin;
    if on_checkin_page {
        page_refresh_attempted = true;
        let before_route = observation.route;
        log.event(
            "page_refresh",
            serde_json::json!({"reason":"checkin_page_at_start","before_route":before_route.as_str(),"action":"back"}),
        )?;
        device.back()?;
        thread::sleep(Duration::from_millis(800));
        // The post-BACK foreground snapshot is load-bearing: BACK can drop the
        // target App out of the foreground entirely (e.g. to the Launcher), and
        // that must be diagnosed as such instead of as a stale in-app page.
        let after_back = log_foreground_snapshot(device, log, "page_refresh_after_back", result)?;
        let mut fresh = observe("page_refresh_after_back")?;
        // BACK can leave the WebView mid-repaint: the first dump then carries
        // only the page title, so the route looks unconfirmed even though the
        // page is fine. Re-observe a bounded number of times before deciding;
        // otherwise a transient repaint would be reported as a failed refresh.
        let mut refresh_attempts = 1u32;
        while should_reobserve(fresh.route, refresh_attempts) {
            log.event(
                "reobserve",
                serde_json::json!({
                    "stage": "page_refresh_after_back",
                    "attempt": refresh_attempts + 1,
                    "status": fresh.status,
                    "clicked": false,
                }),
            )?;
            thread::sleep(UNCONFIRMED_REOBSERVE_INTERVAL);
            fresh = observe("page_refresh_after_back")?;
            refresh_attempts += 1;
        }
        let app_foreground = after_back.as_deref() == Some(target_package);
        page_refresh_confirmed = matches!(fresh.route, PageRoute::Home | PageRoute::Services)
            && fresh.foreign_package.is_none()
            && app_foreground;
        log.event(
            "page_refresh",
            serde_json::json!({"reason":"checkin_page_at_start","before_route":before_route.as_str(),"after_route":fresh.route.as_str(),"after_foreground":after_back,"confirmed":page_refresh_confirmed}),
        )?;
        if !page_refresh_confirmed {
            // Some app builds swallow BACK on the check-in page: verified
            // on-device, repeated keyevents leave the hierarchy byte-identical
            // while the target app stays in the foreground. The refresh is only
            // a convenience that re-enters the page through the catalogue; it
            // is not what makes the observation trustworthy. When the post-BACK
            // observation is still a confirmed check-in page owned by the
            // target app with no foreign window, continue on that page: the run
            // loop re-reads the hierarchy, the foreground package and a fresh
            // screenshot before every decision, and `success` still requires
            // today's date, so a stale success cannot be reported as success.
            let still_checkin = may_continue_on_checkin_page(
                fresh.route,
                fresh.foreign_package.is_some(),
                app_foreground,
            );
            if still_checkin {
                log.event(
                    "page_refresh",
                    serde_json::json!({
                        "reason": "checkin_page_at_start",
                        "before_route": before_route.as_str(),
                        "after_route": fresh.route.as_str(),
                        "after_foreground": after_back,
                        "confirmed": false,
                        "fallback": "continue_on_checkin_page",
                        "clicked": false,
                    }),
                )?;
                continued_on_checkin_page = true;
            } else {
                log.save_bytes("navigation_result.png", &bytes)?;
                let route = stale_refresh_route(fresh.route, after_back.as_deref(), target_package);
                // The App having left the foreground is not a popup overlay: never
                // record the Launcher (or any non-target package) as an overlay.
                let overlay = if route == PageRoute::AppLeftForeground {
                    None
                } else {
                    fresh.foreign_package.clone()
                };
                if route == PageRoute::AppLeftForeground {
                    log.event(
                        "navigation_blocked",
                        serde_json::json!({
                            "reason": "app_left_foreground",
                            "after_foreground": after_back,
                            "route": route.as_str(),
                            "clicked": false,
                        }),
                    )?;
                }
                return Ok(NavigationResult {
                    route,
                    checkin_mode: CheckinMode::Unsupported,
                    field_work_ready: false,
                    page_refresh_attempted,
                    page_refresh_confirmed,
                    hierarchy_status: last_status,
                    overlay_package: overlay,
                });
            }
        }
        observation = fresh;
        bytes = device.screenshot()?;
        image = vision::decode(&bytes)?;
    }

    let initial_route = observation.route;
    let min_entry_y = image.height() / 8;
    let mut navigated = false;

    // 1. Use a hierarchy-confirmed entry only on a known home/services page.
    if matches!(initial_route, PageRoute::Home | PageRoute::Services)
        && let Some((x, y)) = find_checkin_entry_bounds(&observation.view, min_entry_y)
    {
        log.event(
            "navigation_action",
            serde_json::json!({"action":"entry_tap","label":"晚点名签到","x":x,"y":y,"before_route":initial_route.as_str()}),
        )?;
        device.tap(x, y)?;
        thread::sleep(Duration::from_secs(3));
        navigated = true;
        observation = observe("navigation_after_entry_tap")?;
    }

    // 2. Some home layouts put the entry below the fold. Scroll only while the
    //    hierarchy still identifies a known home/services page; never blind-tap
    //    a service-icon coordinate as a fallback.
    if !navigated && matches!(initial_route, PageRoute::Home | PageRoute::Services) {
        for attempt in 0..4 {
            log.event(
                "navigation_action",
                serde_json::json!({"action":"swipe","stage":"home_scroll","attempt":attempt + 1}),
            )?;
            device.swipe(
                image.width() / 2,
                image.height() * 3 / 4,
                image.width() * 2 / 5,
                image.height() * 2 / 5,
                500,
            )?;
            thread::sleep(Duration::from_secs(1));
            observation = observe(&format!("home_scroll_{}", attempt + 1))?;
            current = Some(observation.clone());
            if observation.foreign_package.is_some() {
                break;
            }
            if !is_navigable_catalog_route(observation.route) {
                if observation.route == PageRoute::Checkin {
                    let current_bytes = device.screenshot()?;
                    return finish_navigation(
                        &current_bytes,
                        observation,
                        log,
                        page_refresh_attempted,
                        page_refresh_confirmed,
                        last_status,
                    );
                }
                break;
            }
            if let Some((x, y)) = find_checkin_entry_bounds(&observation.view, min_entry_y) {
                log.event(
                    "navigation_action",
                    serde_json::json!({"action":"entry_tap","label":"晚点名签到","x":x,"y":y,"after":"home_scroll"}),
                )?;
                device.tap(x, y)?;
                thread::sleep(Duration::from_secs(3));
                navigated = true;
                observation = observe("navigation_after_entry_tap")?;
                break;
            }
        }
    }

    // 3. Landscape service catalogues can expose the entry only after opening
    //    the explicitly labelled "业务" tab. Each navigation action is followed
    //    by a fresh observation before any next action.
    if !navigated
        && matches!(observation.route, PageRoute::Home | PageRoute::Services)
        && observation.foreign_package.is_none()
        && let Some((x, y)) = find_clickable_labeled_bounds(&observation.view, "业务", 0)
    {
        log.event(
            "navigation_action",
            serde_json::json!({"action":"tab_tap","label":"业务","x":x,"y":y,"before_route":observation.route.as_str()}),
        )?;
        device.tap(x, y)?;
        thread::sleep(Duration::from_secs(2));
        for attempt in 0..6 {
            observation = observe(&format!("services_scan_{}", attempt + 1))?;
            current = Some(observation.clone());
            if observation.foreign_package.is_some() {
                break;
            }
            if !is_navigable_catalog_route(observation.route) {
                if observation.route == PageRoute::Checkin {
                    let current_bytes = device.screenshot()?;
                    return finish_navigation(
                        &current_bytes,
                        observation,
                        log,
                        page_refresh_attempted,
                        page_refresh_confirmed,
                        last_status,
                    );
                }
                break;
            }
            if let Some((entry_x, entry_y)) =
                find_checkin_entry_bounds(&observation.view, min_entry_y)
            {
                log.event(
                    "navigation_action",
                    serde_json::json!({"action":"entry_tap","label":"晚点名签到","x":entry_x,"y":entry_y,"after":"services_scan"}),
                )?;
                device.tap(entry_x, entry_y)?;
                thread::sleep(Duration::from_secs(3));
                navigated = true;
                observation = observe("navigation_after_entry_tap")?;
                break;
            }
            if attempt < 5 {
                log.event(
                    "navigation_action",
                    serde_json::json!({"action":"swipe","stage":"services_scan","attempt":attempt + 1}),
                )?;
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

    // No accessibility-confirmed navigation target means no navigation action.
    // Do not fall back to a profile coordinate on an unknown page. The last
    // *observed* page identity is reported instead of a bare Unknown, so the
    // diagnosis says "still on the home page" rather than losing that fact.
    //
    // A page that was already the check-in page needs no navigation at all:
    // fall through to the check-in handling below so its mode and button are
    // evaluated by the normal, fail-closed path.
    if !navigated && !continued_on_checkin_page {
        log.save_bytes("navigation_result.png", &bytes)?;
        let last = current.unwrap_or_else(|| observation.clone());
        return Ok(NavigationResult {
            route: last.route,
            checkin_mode: CheckinMode::Unsupported,
            field_work_ready: false,
            page_refresh_attempted,
            page_refresh_confirmed,
            hierarchy_status: last_status,
            overlay_package: last.foreign_package,
        });
    }

    log_foreground_snapshot(device, log, "navigation_after_action", result)?;
    observation = observe("navigation_after_action")?;
    // The route decision below is compared against THIS screenshot, taken in
    // the same round as the observation above; a conflict claim must never be
    // derived from a screenshot that predates the hierarchy read.
    bytes = device.screenshot()?;
    image = vision::decode(&bytes)?;

    if observation.route == PageRoute::Checkin && observation.checkin_mode == CheckinMode::Ordinary
    {
        // The sign-in control must be visibly present before the run loop may
        // interpret its state. Some landscape WebView layouts render the
        // circle below the first viewport, so a bounded re-scroll is allowed;
        // each attempt re-observes the page before continuing.
        let mut visible = visual_candidate(&image, profile).state();
        for attempt in 0..4 {
            if visible != vision::ButtonState::Unknown {
                break;
            }
            if attempt == 3 {
                break;
            }
            log.event(
                "navigation_action",
                serde_json::json!({"action":"swipe","stage":"checkin_scroll","attempt":attempt + 1}),
            )?;
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
            observation = observe(&format!("checkin_scroll_{}", attempt + 1))?;
            if observation.route != PageRoute::Checkin
                || observation.checkin_mode != CheckinMode::Ordinary
            {
                break;
            }
            visible = visual_candidate(&image, profile).state();
        }
        // A confirmed check-in page whose control cannot be seen at all stays
        // an unconfirmed candidate: it never authorizes a tap, and the failure
        // is reported as missing evidence rather than as a page change.
        if observation.route == PageRoute::Checkin && visible == vision::ButtonState::Unknown {
            observation.route = PageRoute::RouteCandidateUnconfirmed;
            observation.checkin_mode = CheckinMode::Unsupported;
        }
    } else if observation.route != PageRoute::Checkin {
        observation.route = unconfirmed_route_for(&image, profile, observation.route);
    }

    finish_navigation(
        &bytes,
        observation,
        log,
        page_refresh_attempted,
        page_refresh_confirmed,
        last_status,
    )
}

/// Classify a non-check-in page against the visual evidence (frozen decision
/// D-08).
///
/// A *detected* control on a page the hierarchy positively identifies as
/// something else is a genuine evidence conflict. Two cases must NOT become a
/// conflict:
/// - the hierarchy could not be read at all, so there is no hierarchy evidence
///   for the screenshot to contradict (`route_candidate_unconfirmed`);
/// - the colour came only from the profile geometry fallback box rather than a
///   detected control, which is weak evidence, not a contradiction.
fn unconfirmed_route_for(
    image: &image::DynamicImage,
    profile: &profile::Profile,
    route: PageRoute,
) -> PageRoute {
    // Nothing to conflict with: an unreadable page stays an unconfirmed
    // candidate no matter what the screenshot shows.
    if route == PageRoute::RouteCandidateUnconfirmed {
        return route;
    }
    // The anti-hijack prompt shows the old check-in page behind it, so a
    // detected control in the screenshot must not turn it into a generic
    // evidence conflict; the dialog itself is the finding.
    if route == PageRoute::AppRestartDialog {
        return route;
    }
    match visual_candidate(image, profile) {
        VisualCandidate::Detected(state) if state != vision::ButtonState::Unknown => {
            PageRoute::RouteEvidenceConflict
        }
        VisualCandidate::GeometryOnly(state) if state != vision::ButtonState::Unknown => {
            if route == PageRoute::Unknown {
                PageRoute::RouteCandidateUnconfirmed
            } else {
                route
            }
        }
        _ => route,
    }
}

/// Map a failed stale-page refresh onto the reason it failed, keeping the
/// "could not observe the result", "observed an unexpected in-app page" and
/// "the App left the foreground" cases distinguishable. None of them is a
/// visual/hierarchy evidence conflict: no screenshot evidence participates in
/// this decision.
///
/// `after_back_foreground` is the post-BACK foreground snapshot: `None` means
/// the query itself failed (unobservable, never "the App is still here"), and a
/// package other than `target_package` means BACK dropped the App out of the
/// foreground. The foreground mismatch is checked first so a Launcher page can
/// never be reported as a stale in-app route.
fn stale_refresh_route(
    fresh_route: PageRoute,
    after_back_foreground: Option<&str>,
    target_package: &str,
) -> PageRoute {
    match after_back_foreground {
        Some(package) if package != target_package => PageRoute::AppLeftForeground,
        // A failed foreground query cannot support "the App is still here", but
        // it also is not evidence that it left; fail closed as unobservable
        // rather than inventing a navigation result.
        None => PageRoute::StalePageRefreshUnconfirmed,
        Some(_) => {
            if fresh_route == PageRoute::AppRestartDialog {
                PageRoute::AppRestartDialog
            } else if fresh_route == PageRoute::RouteCandidateUnconfirmed {
                PageRoute::StalePageRefreshUnconfirmed
            } else {
                PageRoute::StalePageRefreshFailed
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn finish_navigation(
    bytes: &[u8],
    observation: PageObservation,
    log: &RunLog,
    page_refresh_attempted: bool,
    page_refresh_confirmed: bool,
    hierarchy_status: Option<&'static str>,
) -> Result<NavigationResult> {
    let field_work_ready = observation.checkin_mode == CheckinMode::FieldWork
        && observation.view.scoped_text.contains("未签到")
        && observation.view.scoped_text.contains("外勤签到");
    log.save_bytes("navigation_result.png", bytes)?;
    Ok(NavigationResult {
        route: observation.route,
        checkin_mode: observation.checkin_mode,
        field_work_ready,
        page_refresh_attempted,
        page_refresh_confirmed,
        hierarchy_status,
        overlay_package: observation.foreign_package,
    })
}

#[cfg_attr(not(test), expect(dead_code, reason = "test-only label lookup helper"))]
fn find_labeled_bounds(view: &PageView, label: &str, min_center_y: u32) -> Option<(u32, u32)> {
    view.find_labeled(label, min_center_y, false)
}

fn find_clickable_labeled_bounds(
    view: &PageView,
    label: &str,
    min_center_y: u32,
) -> Option<(u32, u32)> {
    view.find_labeled(label, min_center_y, true)
}

/// The home/services entry that opens the check-in page.
///
/// The entry label is the same string as the check-in page's own title, so on a
/// page that is *already* the check-in page this lookup would return the title
/// node's coordinates -- a click that does nothing useful and could scroll the
/// live page. The entry is therefore only ever resolved on a page that is not
/// the check-in page.
fn find_checkin_entry_bounds(view: &PageView, min_center_y: u32) -> Option<(u32, u32)> {
    if detect_page_route(view) == PageRoute::Checkin {
        return None;
    }
    find_clickable_labeled_bounds(view, CHECKIN_ENTRY_LABEL, min_center_y)
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
        GIT_COMMIT, PageRoute, PageView, RunStatus, SyncLifecycle, TapDevice, TapRequest, VERSION,
        VisualCandidate, blocked_instance_result, build_id, build_id_for, build_task_command_line,
        detect_page_route, finalize_run, find_checkin_entry_bounds, find_clickable_labeled_bounds,
        find_labeled_bounds, foreground_check, git_dirty, guarded_tap, instance_summary_entry,
        location_wait_limit, may_continue_on_checkin_page, new_run_result, propagated_failure,
        restore_sync_backup, set_run_failure, unconfirmed_page_failure, unconfirmed_route_for,
        visual_candidate,
    };
    use crate::{
        config::{Config, EmulatorInstanceConfig},
        domain::{CheckinMode, Mode, RunResult},
        logger::RunLog,
        vision::ButtonState,
    };
    use anyhow::Result;
    // `Parser` provides `try_parse_from`, used by the CLI argument tests.
    use clap::CommandFactory;
    use clap::Parser;
    use std::{
        cell::{Cell, RefCell},
        fs,
        path::Path,
        time::Duration,
    };

    /// Test-only package scope for hand-written hierarchy fixtures.
    const TEST_PACKAGE: &str = "cn.edu.fzu.fdxypa";

    /// Success-date parsing in tests always scopes to the target package,
    /// matching the production call site.
    fn parse_success_date(xml: &str) -> Option<chrono::NaiveDate> {
        super::extract_hierarchy_date(xml, TEST_PACKAGE)
    }

    /// Parse a hand-written XML fixture the way the run loop does. Fixtures
    /// without a `package` attribute stay in scope, so existing expectations
    /// are unchanged by package scoping.
    fn page(xml: &str) -> PageView {
        PageView::parse(xml, TEST_PACKAGE)
    }

    fn temp_root(label: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("zhfd-{label}-{}-{nonce}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn fixture_image(name: &str) -> image::DynamicImage {
        let bytes: &[u8] = match name {
            "ready" => include_bytes!("../../autojs6/fixtures/ready.png"),
            "gray" => include_bytes!("../../autojs6/fixtures/gray.png"),
            _ => panic!("unknown fixture"),
        };
        crate::vision::decode(bytes).expect("fixture decode")
    }

    // ===== v0.3.0 route-evidence semantics (frozen decision D-08) =====

    #[test]
    fn detected_button_on_non_checkin_page_is_a_real_evidence_conflict() {
        // A saturated sign-in control genuinely found in the screenshot while
        // the hierarchy says the page is something else is a contradiction.
        let image = fixture_image("ready");
        assert!(matches!(
            visual_candidate(&image, test_profile()),
            VisualCandidate::Detected(_)
        ));
        assert_eq!(
            unconfirmed_route_for(&image, test_profile(), PageRoute::Home),
            PageRoute::RouteEvidenceConflict
        );
    }

    #[test]
    fn locating_wait_is_capped_at_the_frozen_120_seconds() {
        let root = temp_root("locating-cap");
        let mut config = Config::default();
        config.runtime.location_timeout_seconds = 600;
        let store = crate::config::ConfigStore {
            root: root.clone(),
            path: root.join("config.toml"),
            config,
            origin: crate::config::ConfigOrigin::Explicit,
            created_default: false,
            recovery_error: None,
        };
        assert_eq!(
            location_wait_limit(&store),
            120,
            "配置不得把定位等待放宽到超过 D-09 的 120 秒硬上限"
        );

        let mut shorter = store.clone();
        shorter.config.runtime.location_timeout_seconds = 30;
        assert_eq!(location_wait_limit(&shorter), 30);

        // A zero/absurd value must not disable the wait entirely.
        let mut zero = store.clone();
        zero.config.runtime.location_timeout_seconds = 0;
        assert_eq!(location_wait_limit(&zero), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unreadable_hierarchy_stays_an_unconfirmed_candidate_even_with_visual_evidence() {
        // D-08: when the hierarchy cannot be read there is no hierarchy evidence
        // for the screenshot to contradict, so a visually plausible check-in
        // control must NOT be reported as `route_evidence_conflict`.
        let image = fixture_image("ready");
        let profile = test_profile();
        assert!(matches!(
            visual_candidate(&image, profile),
            VisualCandidate::Detected(_)
        ));
        assert_eq!(
            unconfirmed_route_for(&image, profile, PageRoute::RouteCandidateUnconfirmed),
            PageRoute::RouteCandidateUnconfirmed,
            "层级不可读时不得报告证据冲突"
        );
    }

    #[test]
    fn geometry_only_fallback_colour_must_not_report_evidence_conflict() {
        // A flat gray screen has no detectable sign-in control; only the
        // profile geometry fallback box has any signal at all. That is weak
        // evidence, not a hierarchy/visual contradiction.
        let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            900,
            1600,
            image::Rgb([150, 150, 150]),
        ));
        let profile = test_profile();
        assert!(crate::vision::find_colored_button(&image).is_none());
        assert_eq!(
            visual_candidate(&image, profile),
            VisualCandidate::GeometryOnly(ButtonState::Gray)
        );
        assert_eq!(
            unconfirmed_route_for(&image, profile, PageRoute::Unknown),
            PageRoute::RouteCandidateUnconfirmed,
            "fallback geometry must not be promoted to route_evidence_conflict"
        );
        assert_eq!(
            unconfirmed_route_for(&image, profile, PageRoute::Home),
            PageRoute::Home,
            "a known non-checkin route must not be overwritten by fallback geometry"
        );
    }

    #[test]
    fn unconfirmed_and_conflict_routes_can_never_authorize_a_tap() {
        for route in [
            PageRoute::RouteCandidateUnconfirmed,
            PageRoute::RouteEvidenceConflict,
        ] {
            let device = MockTapDevice::default();
            let mut request = request(ButtonState::Ready);
            request.route = route;
            assert!(
                guarded_tap(&device, request).is_err(),
                "{route:?} must not authorize a tap"
            );
            assert!(device.taps.borrow().is_empty());
            assert_eq!(route.error_code(), Some(route.as_str()));
        }
    }

    #[test]
    fn route_error_codes_are_stable_and_checkin_has_none() {
        assert_eq!(PageRoute::Login.error_code(), Some("login_required"));
        for route in [PageRoute::Home, PageRoute::Services, PageRoute::Unknown] {
            assert_eq!(route.error_code(), Some("navigation_not_confirmed"));
        }
        assert_eq!(PageRoute::Checkin.error_code(), None);
    }

    /// `unsupported_checkin` used to be a route. It was removed because a route
    /// is page identity only (D-08) and the check-in *kind* belongs to
    /// `checkin_mode`; the variant had no producer and only shadowed the
    /// check-in page's real title evidence. Historical logs that still carry the
    /// string must keep loading.
    #[test]
    fn the_retired_unsupported_checkin_route_still_deserializes_as_unknown() {
        let retired: PageRoute = serde_json::from_str(r#""unsupported_checkin""#).unwrap();
        assert_eq!(retired, PageRoute::Unknown);
        assert_eq!(retired.error_code(), Some("navigation_not_confirmed"));

        for route in [
            PageRoute::Login,
            PageRoute::Home,
            PageRoute::Services,
            PageRoute::Checkin,
            PageRoute::RouteCandidateUnconfirmed,
            PageRoute::RouteEvidenceConflict,
            PageRoute::StalePageRefreshFailed,
            PageRoute::StalePageRefreshUnconfirmed,
            PageRoute::AppLeftForeground,
            PageRoute::Unknown,
        ] {
            let text = serde_json::to_string(&route).unwrap();
            assert_eq!(serde_json::from_str::<PageRoute>(&text).unwrap(), route);
        }
    }

    // ===== v0.3.0 result convergence (frozen decisions D-05/D-07) =====

    #[test]
    fn set_run_failure_populates_the_stable_machine_code() {
        let root = temp_root("run-failure");
        let log = RunLog::new(&root).unwrap();
        let mut result = new_run_result(Mode::DryRun, &log, &super::InstanceTargetArgs::default());
        assert_eq!(result.status.as_deref(), Some("running"));
        assert!(result.error_code.is_none());

        set_run_failure(&mut result, "outside_window", "time_window", "请等待窗口");
        assert_eq!(result.error_code.as_deref(), Some("outside_window"));
        assert_eq!(result.error.as_deref(), Some("outside_window"));
        assert_eq!(result.error_stage.as_deref(), Some("time_window"));
        assert!(result.recovery_action.is_some());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn finalize_run_converges_running_into_a_terminal_status() {
        let root = temp_root("finalize");
        let log = RunLog::new(&root).unwrap();

        let mut ready = new_run_result(Mode::DryRun, &log, &super::InstanceTargetArgs::default());
        ready.dry_run_ready = true;
        finalize_run(&log, &mut ready).unwrap();
        assert_eq!(ready.status.as_deref(), Some(RunStatus::Success.as_str()));
        assert!(ready.finished_at.is_some());
        assert!(
            ready
                .artifact_paths
                .iter()
                .any(|path| path.ends_with("result.json")),
            "result.json must be referenced as an artifact"
        );

        let mut failed = new_run_result(Mode::DryRun, &log, &super::InstanceTargetArgs::default());
        set_run_failure(&mut failed, "unknown_state", "visual_observation", "重试");
        finalize_run(&log, &mut failed).unwrap();
        assert_eq!(failed.status.as_deref(), Some(RunStatus::Failed.as_str()));
        assert_eq!(failed.error_code.as_deref(), Some("unknown_state"));

        // `blocked` is owned by the batch layer and must survive finalization.
        let mut blocked = new_run_result(Mode::DryRun, &log, &super::InstanceTargetArgs::default());
        blocked.status = Some(RunStatus::Blocked.as_str().into());
        finalize_run(&log, &mut blocked).unwrap();
        assert_eq!(blocked.status.as_deref(), Some(RunStatus::Blocked.as_str()));

        let _ = fs::remove_dir_all(root);
    }

    // ===== v0.3.0 batch coordination (frozen decisions D-05/D-07) =====

    #[test]
    fn blocked_instance_result_keeps_identity_and_shared_blocker() {
        let root = temp_root("blocked");
        let log = RunLog::new(&root).unwrap();
        let instance = EmulatorInstanceConfig {
            instance_index: 2,
            name: "签到实例 2".into(),
            serial: String::new(),
            enabled: true,
        };
        let result = blocked_instance_result(Mode::DryRun, &log, &instance, "ldconsole 不可用");

        assert_eq!(result.status.as_deref(), Some(RunStatus::Blocked.as_str()));
        assert_eq!(result.blocked_by.as_deref(), Some("shared_preflight"));
        assert_eq!(
            result.error_code.as_deref(),
            Some("shared_preflight_failed")
        );
        assert_eq!(result.error_stage.as_deref(), Some("shared_preflight"));
        assert_eq!(result.instance_index, Some(2));
        assert_eq!(result.instance_name.as_deref(), Some("签到实例 2"));
        // The serial is derived from the TCP ADB port for the index when the
        // configuration does not pin one, so the summary entry stays usable.
        assert_eq!(result.serial, "127.0.0.1:5559");
        assert!(!result.clicked);
        assert!(!result.success);

        let entry = instance_summary_entry(&result, RunStatus::Blocked, None);
        assert_eq!(entry["index"], 2);
        assert_eq!(entry["name"], "签到实例 2");
        assert_eq!(entry["serial"], "127.0.0.1:5559");
        assert_eq!(entry["status"], "blocked");
        assert_eq!(entry["blocked_by"], "shared_preflight");
        assert_eq!(entry["error_code"], "shared_preflight_failed");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn batch_summary_is_written_under_a_batch_scoped_directory() {
        let root = temp_root("batch-summary");
        let summary = serde_json::json!({"schema_version": 3, "status": "blocked"});
        let path =
            crate::logger::write_batch_summary(&root, "2026-10-14_221000_000_1", &summary).unwrap();
        assert_eq!(
            path,
            root.join("logs")
                .join("batch-2026-10-14_221000_000_1")
                .join("summary.json")
        );
        let written: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["status"], "blocked");

        // A second batch must not overwrite the first one's summary.
        let other =
            crate::logger::write_batch_summary(&root, "2026-10-14_221500_000_2", &summary).unwrap();
        assert_ne!(other, path);
        assert!(path.is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn run_result_route_and_page_route_stay_in_lockstep() {
        let root = temp_root("route-lockstep");
        let log = RunLog::new(&root).unwrap();
        let mut result: RunResult =
            new_run_result(Mode::DryRun, &log, &super::InstanceTargetArgs::default());
        assert!(result.route.is_none());
        super::set_page_route(&mut result, PageRoute::Checkin);
        assert_eq!(result.route.as_deref(), Some("checkin"));
        assert_eq!(result.page_route, Some(PageRoute::Checkin));
        let _ = fs::remove_dir_all(root);
    }

    /// The profile only supplies the fallback geometry ratios, so the tests pin
    /// one explicitly instead of depending on a fixture-to-profile match.
    fn test_profile() -> &'static crate::profile::Profile {
        crate::profile::find_profile_by_id("portrait_900x1600_d320")
            .expect("built-in profile must exist")
    }

    // ===== v0.3.0 CLI config path (frozen decision D-01) =====

    #[test]
    fn global_config_flag_is_accepted_before_and_after_the_subcommand() {
        let cli = super::Cli::try_parse_from([
            "zhfd-checkin",
            "--config",
            r"C:\User Data\ZHFD\config.toml",
            "run",
            "--dry-run",
        ])
        .expect("--config 必须可作为全局参数出现在子命令之前");
        assert_eq!(
            cli.config.as_deref(),
            Some(Path::new(r"C:\User Data\ZHFD\config.toml"))
        );

        let cli = super::Cli::try_parse_from([
            "zhfd-checkin",
            "run",
            "--dry-run",
            "--config",
            r"C:\User Data\ZHFD\config.toml",
        ])
        .expect("--config 也必须可用于子命令之后");
        assert_eq!(
            cli.config.as_deref(),
            Some(Path::new(r"C:\User Data\ZHFD\config.toml"))
        );

        // Without the flag the default path-selection chain applies.
        let cli = super::Cli::try_parse_from(["zhfd-checkin", "run", "--dry-run"]).unwrap();
        assert!(cli.config.is_none());
    }

    #[test]
    fn run_rejects_conflicting_mode_and_batch_selectors() {
        for args in [
            vec!["zhfd-checkin", "run", "--dry-run", "--confirm"],
            vec![
                "zhfd-checkin",
                "run",
                "--all-instances",
                "--instance-index",
                "0",
            ],
            vec![
                "zhfd-checkin",
                "run",
                "--all-instances",
                "--serial",
                "127.0.0.1:5555",
            ],
        ] {
            assert!(
                super::Cli::try_parse_from(&args).is_err(),
                "应拒绝冲突参数: {args:?}"
            );
        }
        assert!(super::Cli::try_parse_from(["zhfd-checkin", "run", "--dry-run"]).is_ok());
        assert!(super::Cli::try_parse_from(["zhfd-checkin", "run", "--live", "--confirm"]).is_ok());
    }

    #[test]
    fn task_create_rejects_batch_selector_combination() {
        for args in [
            vec![
                "zhfd-checkin",
                "task",
                "create",
                "--all-instances",
                "--instance-index",
                "0",
            ],
            vec![
                "zhfd-checkin",
                "task",
                "create",
                "--all-instances",
                "--serial",
                "127.0.0.1:5555",
            ],
        ] {
            assert!(
                super::Cli::try_parse_from(&args).is_err(),
                "计划任务不能同时指定 --all-instances 与实例选择器: {args:?}"
            );
        }
        assert!(
            super::Cli::try_parse_from(["zhfd-checkin", "task", "create", "--all-instances"])
                .is_ok()
        );
    }

    #[test]
    fn app_install_and_task_delete_require_an_explicit_confirm_flag() {
        assert!(
            super::Cli::try_parse_from(["zhfd-checkin", "app", "install", "--apk", "app.apk",])
                .is_err(),
            "app install 必须在 clap 层要求 --confirm"
        );
        assert!(
            super::Cli::try_parse_from([
                "zhfd-checkin",
                "app",
                "install",
                "--apk",
                "app.apk",
                "--confirm",
            ])
            .is_ok()
        );
        assert!(
            super::Cli::try_parse_from(["zhfd-checkin", "task", "delete"]).is_err(),
            "task delete 必须在 clap 层要求 --confirm"
        );
        assert!(
            super::Cli::try_parse_from(["zhfd-checkin", "task", "delete", "--confirm"]).is_ok()
        );
    }

    #[test]
    fn task_create_accepts_dry_run_and_rejects_a_mode_conflict() {
        assert!(
            super::Cli::try_parse_from(["zhfd-checkin", "task", "create", "--dry-run"]).is_ok()
        );
        assert!(
            super::Cli::try_parse_from([
                "zhfd-checkin",
                "task",
                "create",
                "--dry-run",
                "--live",
                "--confirm",
            ])
            .is_err(),
            "--dry-run 与 --live 不能同时用于计划任务"
        );
    }

    #[test]
    fn run_json_flag_and_top_level_aliases_parse() {
        assert!(super::Cli::try_parse_from(["zhfd-checkin", "run", "--dry-run", "--json"]).is_ok());
        assert!(super::Cli::try_parse_from(["zhfd-checkin", "doctor"]).is_ok());
        assert!(
            super::Cli::try_parse_from(
                ["zhfd-checkin", "analyze-image", "--image", "fixture.png",]
            )
            .is_ok()
        );
    }

    #[test]
    fn login_command_parses_and_has_no_profile_requirement() {
        let cli = super::Cli::try_parse_from(["zhfd-checkin", "login"]).expect("login");
        match cli.command {
            Some(super::CommandKind::Login(args)) => {
                assert!(!args.confirm_login);
                assert!(!args.json);
            }
            other => panic!("unexpected command: {other:?}"),
        }
        let cli = super::Cli::try_parse_from([
            "zhfd-checkin",
            "login",
            "--confirm-login",
            "--instance-index",
            "0",
        ])
        .expect("login --confirm-login");
        match cli.command {
            Some(super::CommandKind::Login(args)) => {
                assert!(args.confirm_login);
                assert_eq!(args.target.instance_index, Some(0));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn login_only_results_converge_to_success_only_when_login_succeeded() {
        let root = temp_root("login-only-finalize");
        let log = RunLog::new(&root).unwrap();

        let mut probe = new_run_result(Mode::DryRun, &log, &super::InstanceTargetArgs::default());
        probe.login_only = Some(true);
        probe.auto_login_ready = Some(true);
        finalize_run(&log, &mut probe).unwrap();
        assert_eq!(probe.status.as_deref(), Some(RunStatus::Success.as_str()));
        assert!(!probe.clicked);

        // A refused login-only action carries an error code and stays failed.
        let mut refused = new_run_result(Mode::Live, &log, &super::InstanceTargetArgs::default());
        refused.login_only = Some(true);
        refused.auto_login_attempted = Some(false);
        set_run_failure(&mut refused, "auto_login_disabled", "auto_login", "未启用");
        finalize_run(&log, &mut refused).unwrap();
        assert_eq!(refused.status.as_deref(), Some(RunStatus::Failed.as_str()));

        let mut logged_in = new_run_result(Mode::Live, &log, &super::InstanceTargetArgs::default());
        logged_in.login_only = Some(true);
        logged_in.auto_login_attempted = Some(true);
        logged_in.auto_login_succeeded = Some(true);
        logged_in.auto_login_tap_count = Some(1);
        finalize_run(&log, &mut logged_in).unwrap();
        assert_eq!(
            logged_in.status.as_deref(),
            Some(RunStatus::Success.as_str())
        );
        // A login tap is never reported as a check-in tap.
        assert!(!logged_in.clicked);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn profile_sync_requires_an_explicit_confirm_flag() {
        assert!(
            super::Cli::try_parse_from([
                "zhfd-checkin",
                "profile",
                "sync",
                "--id",
                "portrait_900x1600_d320",
            ])
            .is_err(),
            "profile sync 必须在 clap 层要求 --confirm"
        );
        assert!(
            super::Cli::try_parse_from([
                "zhfd-checkin",
                "profile",
                "sync",
                "--id",
                "portrait_900x1600_d320",
                "--confirm",
            ])
            .is_ok()
        );
    }

    #[test]
    fn every_top_level_command_has_help_about_and_run_mode_help() {
        let mut command = super::Cli::command();
        command.build();
        for subcommand in command.get_subcommands() {
            if subcommand.get_name() == "help" {
                continue;
            }
            assert!(
                subcommand.get_about().is_some(),
                "子命令 {} 缺少 about",
                subcommand.get_name()
            );
        }
        let run = command
            .find_subcommand_mut("run")
            .expect("run subcommand must exist");
        run.build();
        for id in ["dry_run", "live", "confirm", "all_instances"] {
            let arg = run
                .get_arguments()
                .find(|arg| arg.get_id() == id)
                .unwrap_or_else(|| panic!("run 缺少参数 {id}"));
            assert!(arg.get_help().is_some(), "run --{id} 缺少 help");
        }
    }

    #[test]
    fn config_path_subcommand_parses() {
        let cli =
            super::Cli::try_parse_from(["zhfd-checkin", "config", "path"]).expect("config path");
        assert!(matches!(
            cli.command,
            Some(super::CommandKind::Config {
                command: super::CliConfigCommand::Path
            })
        ));
    }

    #[test]
    fn config_path_works_with_a_damaged_config() {
        let root = temp_root("cli-config-path-damaged");
        let path = root.join("config.toml");
        fs::write(&path, "this is not toml").unwrap();
        let cli = super::Cli::try_parse_from([
            "zhfd-checkin",
            "--config",
            path.to_str().unwrap(),
            "config",
            "path",
        ])
        .unwrap();
        assert!(super::run_cli(cli).is_ok());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn profile_list_does_not_require_a_valid_config() {
        let root = temp_root("cli-profile-list-no-config");
        let path = root.join("config.toml");
        fs::write(&path, "this is not toml").unwrap();
        let cli = super::Cli::try_parse_from([
            "zhfd-checkin",
            "--config",
            path.to_str().unwrap(),
            "profile",
            "list",
        ])
        .unwrap();
        assert!(super::run_cli(cli).is_ok());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn damaged_config_makes_cli_fail_with_the_path_and_a_repair_pointer() {
        let root = temp_root("cli-damaged-config");
        let path = root.join("config.toml");
        fs::write(&path, "this is not toml").unwrap();

        let error = super::load_store_for_cli(Some(&path)).unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains(&path.display().to_string()),
            "错误信息必须给出实际配置路径: {message}"
        );
        assert!(
            message.contains("zhfd-checkin-gui.exe"),
            "错误信息必须指向 GUI Setup/Repair 入口: {message}"
        );
        assert!(
            message.contains("不会自动恢复默认配置"),
            "CLI 不得暗示会静默恢复默认配置: {message}"
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "this is not toml",
            "CLI 失败不得改写损坏的配置"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn default_config_path_matches_the_loader_candidate_order() {
        // The Repair page and the CLI error message must name the file the
        // loader actually tried. With no explicit path and no portable config
        // present, the first candidate is the portable one beside the EXE.
        let path = crate::config::ConfigStore::default_config_path().unwrap();
        assert_eq!(path.file_name().unwrap(), "config.toml");
        let exe_root = crate::config::exe_root().unwrap();
        // Either the portable candidate (no LocalAppData config) or the
        // LocalAppData candidate, never something unrelated.
        let local = crate::config::recommended_local_app_data_root();
        assert!(
            path == exe_root.join("config.toml")
                || local.is_some_and(|root| path == root.join("config.toml")),
            "recovery 路径必须来自加载候选链: {}",
            path.display()
        );
    }

    #[test]
    fn cli_config_display_path_prefers_the_explicit_value() {
        assert_eq!(
            super::describe_config_path(Some(Path::new(r"C:\x\config.toml"))),
            r"C:\x\config.toml"
        );
        // Without an explicit path the portable candidate is reported.
        assert!(
            super::describe_config_path(None).ends_with("config.toml"),
            "默认路径提示必须指向某个 config.toml"
        );
    }

    #[test]
    fn propagated_failures_keep_a_stage_specific_machine_code() {
        assert_eq!(propagated_failure("device_setup").0, "device_setup_failed");
        assert_eq!(propagated_failure("app_launch").0, "app_start_failed");
        assert_eq!(propagated_failure("app_foreground").0, "app_start_failed");
        assert_eq!(
            propagated_failure("visual_observation").0,
            "observation_failed"
        );
        assert_eq!(propagated_failure("anything_else").0, "runtime_error");
        // Every branch must carry actionable recovery text.
        for stage in [
            "device_setup",
            "app_launch",
            "visual_observation",
            "runtime",
        ] {
            let (_, recovery) = propagated_failure(stage);
            assert!(!recovery.trim().is_empty(), "stage {stage} 缺少恢复建议");
        }
    }

    // ===== popup / advertisement blocking the page (v0.3.0 follow-up) =====

    #[test]
    fn a_foreign_window_is_detected_and_excluded_from_page_identity() {
        // The app owns the foreground package, but a second process has drawn a
        // window on top of it. Its words must not be able to impersonate the
        // check-in page.
        let xml = concat!(
            r#"<hierarchy>"#,
            r#"<node index="0" package="com.example.ad" text="限时活动 立即领取" bounds="[100,600][800,900]"/>"#,
            r#"<node index="0" package="cn.edu.fzu.fdxypa" text="晚点名签到" bounds="[0,0][900,100]"/>"#,
            r#"</hierarchy>"#
        );
        let view = PageView::parse(xml, TEST_PACKAGE);
        assert_eq!(view.foreign_package(), Some("com.example.ad"));
        // Only the target app's nodes carry page identity, so the sweepstake
        // copy cannot become a route marker.
        assert!(!view.scoped_text.contains("限时活动"));
        assert!(view.scoped_text.contains("晚点名签到"));
    }

    #[test]
    fn a_sweepstake_popup_alone_never_confirms_the_checkin_page() {
        // Even if the advertisement reuses the page's own words, a route must
        // not be confirmed from another process's window.
        let xml = concat!(
            r#"<hierarchy>"#,
            r#"<node package="com.example.ad" text="晚点名签到"/>"#,
            r#"<node package="com.example.ad" text="点击签到"/>"#,
            r#"</hierarchy>"#
        );
        let view = PageView::parse(xml, TEST_PACKAGE);
        assert_eq!(view.foreign_package(), Some("com.example.ad"));
        assert_eq!(
            detect_page_route(&view),
            PageRoute::Unknown,
            "外进程窗口不得确认签到路由"
        );
        assert!(matches!(
            unconfirmed_page_failure(view.foreign_package()),
            ("overlay_present", _)
        ));
    }

    /// `unsupported_checkin` was retired as a route; `checkin_mode` already
    /// carries that distinction on a confirmed check-in page, and the route had
    /// no producer. The entry card's label and the check-in page's title are the
    /// same string, so the retired variant only ever shadowed real page
    /// evidence -- the home page's entry card must stay `Home`, and a
    /// field-work-only page must stay unidentified rather than masquerade as a
    /// check-in page.
    #[test]
    fn the_checkin_entry_card_is_never_mistaken_for_the_checkin_page() {
        assert_eq!(
            detect_page_route(&page(r#"<node text="我的服务"/><node text="晚点名签到"/>"#)),
            PageRoute::Home
        );
        assert_eq!(
            detect_page_route(&page(
                r#"<node text="外勤签到"/><node text="当前位置"/><node text="签到时间"/>"#
            )),
            PageRoute::Unknown
        );
    }

    /// The check-in page's own title node is clickable, and its label is the
    /// same string as the home entry card. Resolving the entry on a page that is
    /// already the check-in page would click the title, so it must return
    /// nothing there.
    #[test]
    fn the_checkin_entry_is_only_resolved_off_the_checkin_page() {
        let checkin_page = page(concat!(
            r#"<node text="晚点名签到" clickable="true" bounds="[0,300][900,400]"/>"#,
            r#"<node text="点击签到" clickable="true" bounds="[0,900][900,1000]"/>"#
        ));
        assert_eq!(detect_page_route(&checkin_page), PageRoute::Checkin);
        assert_eq!(find_checkin_entry_bounds(&checkin_page, 100), None);

        let home_page = page(concat!(
            r#"<node text="我的服务" bounds="[0,0][900,100]"/>"#,
            r#"<node text="晚点名签到" clickable="true" bounds="[0,600][900,700]"/>"#
        ));
        assert_eq!(detect_page_route(&home_page), PageRoute::Home);
        assert_eq!(find_checkin_entry_bounds(&home_page, 100), Some((450, 650)));
    }

    #[test]
    fn build_id_marks_a_dirty_or_undetermined_tree() {
        // FR-014: `git_commit` alone would claim a provenance the binary may not
        // have, so a dirty (or undeterminable) build must be visibly marked.
        assert_eq!(
            build_id_for("0.3.0", "abcdef123456", Some(false)),
            "0.3.0-abcdef123456"
        );
        assert_eq!(
            build_id_for("0.3.0", "abcdef123456", Some(true)),
            "0.3.0-abcdef123456+dirty"
        );
        assert_eq!(
            build_id_for("0.3.0", "unknown", None),
            "0.3.0-unknown+unknown"
        );
        // The build that produced this test binary must be internally
        // consistent: the id and the flag come from the same build-script run.
        assert_eq!(build_id(), build_id_for(VERSION, GIT_COMMIT, git_dirty()));
    }

    #[test]
    fn unconfirmed_page_failure_separates_overlay_from_missing_markers() {
        let (code, recovery) = unconfirmed_page_failure(Some("com.example.ad"));
        assert_eq!(code, "overlay_present");
        assert!(
            recovery.contains("com.example.ad"),
            "恢复建议必须指出遮挡窗口的来源: {recovery}"
        );
        assert!(recovery.contains("未执行任何点击"));

        let (code, recovery) = unconfirmed_page_failure(None);
        assert_eq!(code, "route_candidate_unconfirmed");
        assert!(!recovery.is_empty());
    }

    #[test]
    fn overlay_does_not_authorize_the_stale_page_back_action() {
        // A stale check-in page is normally refreshed with BACK. With a popup
        // present there is no way to know which window BACK would act on, so
        // the decision must stop instead of pressing a key.
        let xml = concat!(
            r#"<hierarchy>"#,
            r#"<node package="com.example.ad" text="公告" bounds="[100,600][800,900]"/>"#,
            r#"<node package="cn.edu.fzu.fdxypa" text="晚点名签到" bounds="[0,0][900,100]"/>"#,
            r#"<node package="cn.edu.fzu.fdxypa" text="点击签到" bounds="[0,100][900,200]"/>"#,
            r#"</hierarchy>"#
        );
        let view = PageView::parse(xml, TEST_PACKAGE);
        assert_eq!(detect_page_route(&view), PageRoute::Checkin);
        assert_eq!(view.foreign_package(), Some("com.example.ad"));
    }

    #[test]
    fn fixtures_without_a_package_attribute_stay_in_scope() {
        // Existing hand-written fixtures and partial dumps omit `package`; the
        // scoping must only ever remove foreign evidence.
        let view = page(r#"<node text="晚点名签到"/><node text="无法签到"/>"#);
        assert!(view.packages.is_empty());
        assert_eq!(view.foreign_package(), None);
        assert_eq!(detect_page_route(&view), PageRoute::Checkin);
    }

    #[test]
    fn unconfirmed_reobserve_is_bounded_and_only_for_unidentified_pages() {
        // Only an unidentified page is re-read, and only a bounded number of
        // times: a persistently blocked page must fail rather than loop.
        assert!(super::should_reobserve(
            PageRoute::RouteCandidateUnconfirmed,
            1
        ));
        assert!(!super::should_reobserve(
            PageRoute::RouteCandidateUnconfirmed,
            super::UNCONFIRMED_REOBSERVE_ATTEMPTS
        ));
        for route in [
            PageRoute::Checkin,
            PageRoute::Home,
            PageRoute::Services,
            PageRoute::Login,
            PageRoute::Unknown,
            PageRoute::RouteEvidenceConflict,
            PageRoute::StalePageRefreshFailed,
            PageRoute::StalePageRefreshUnconfirmed,
            PageRoute::AppLeftForeground,
        ] {
            assert!(
                !super::should_reobserve(route, 1),
                "{route:?} 不应触发重新观测"
            );
        }
        assert!(super::UNCONFIRMED_REOBSERVE_INTERVAL >= Duration::from_millis(1));
    }

    #[test]
    fn stale_refresh_failures_are_not_reported_as_evidence_conflicts() {
        // Returning from an old check-in page is a navigation decision: no
        // screenshot evidence is involved, so no outcome may claim a
        // visual/hierarchy conflict.
        const TARGET: &str = "cn.edu.fzu.fdxypa";
        assert_eq!(
            super::stale_refresh_route(PageRoute::RouteCandidateUnconfirmed, Some(TARGET), TARGET),
            PageRoute::StalePageRefreshUnconfirmed
        );
        for route in [
            PageRoute::Home,
            PageRoute::Services,
            PageRoute::Login,
            PageRoute::Unknown,
        ] {
            assert_eq!(
                super::stale_refresh_route(route, Some(TARGET), TARGET),
                PageRoute::StalePageRefreshFailed,
                "{route:?} 必须归类为 stale_page_refresh_failed"
            );
        }
        // A failed foreground query is unobservable, never "still in the App".
        for route in [PageRoute::RouteCandidateUnconfirmed, PageRoute::Home] {
            assert_eq!(
                super::stale_refresh_route(route, None, TARGET),
                PageRoute::StalePageRefreshUnconfirmed,
                "{route:?} 在前台查询失败时必须归类为 stale_page_refresh_unconfirmed"
            );
        }
        // A different package means BACK dropped the App out of the foreground:
        // distinct from both stale in-app outcomes for *every* fresh route,
        // including the unconfirmed one that a Launcher dump produces.
        for route in [
            PageRoute::RouteCandidateUnconfirmed,
            PageRoute::Home,
            PageRoute::Services,
            PageRoute::Login,
            PageRoute::Unknown,
            PageRoute::Checkin,
        ] {
            assert_eq!(
                super::stale_refresh_route(route, Some("com.android.launcher3"), TARGET),
                PageRoute::AppLeftForeground,
                "{route:?} 在目标 App 离开前台时必须归类为 app_left_foreground"
            );
        }
        assert_eq!(
            PageRoute::StalePageRefreshFailed.error_code(),
            Some("stale_page_refresh_failed")
        );
        assert_eq!(
            PageRoute::StalePageRefreshUnconfirmed.error_code(),
            Some("stale_page_refresh_unconfirmed")
        );
        // The app-left route reuses the FR-013 code, so GUI/CLI recovery stays
        // correct without a third synonym for the same condition.
        assert_eq!(
            PageRoute::AppLeftForeground.error_code(),
            Some("target_app_not_foreground")
        );
        assert_eq!(PageRoute::AppLeftForeground.as_str(), "app_left_foreground");
    }

    #[test]
    fn a_checkin_page_that_swallowed_back_may_still_be_used() {
        // Some app builds ignore BACK on the check-in page; the page is then
        // still a legitimate, fully re-observed check-in page.
        assert!(may_continue_on_checkin_page(
            PageRoute::Checkin,
            false,
            true
        ));
        // Every other outcome keeps the existing fail-closed paths.
        for route in [
            PageRoute::Home,
            PageRoute::Services,
            PageRoute::Login,
            PageRoute::Unknown,
            PageRoute::RouteCandidateUnconfirmed,
            PageRoute::StalePageRefreshFailed,
            PageRoute::AppLeftForeground,
        ] {
            assert!(
                !may_continue_on_checkin_page(route, false, true),
                "{route:?} 不得被当作可继续的签到页"
            );
        }
        // A foreign window or a lost foreground is never usable.
        assert!(!may_continue_on_checkin_page(
            PageRoute::Checkin,
            true,
            true
        ));
        assert!(!may_continue_on_checkin_page(
            PageRoute::Checkin,
            false,
            false
        ));
    }

    #[test]
    fn foreground_check_separates_a_missing_app_from_an_unreadable_query() {
        // Confirmed target App: no failure.
        assert_eq!(
            foreground_check(Some("cn.edu.fzu.fdxypa"), "cn.edu.fzu.fdxypa"),
            None
        );
        // A different package is its own diagnosis with its own recovery.
        assert_eq!(
            foreground_check(Some("com.android.launcher"), "cn.edu.fzu.fdxypa")
                .map(|(code, _)| code),
            Some("target_app_not_foreground")
        );
        // A failed query must never be read as "still in the foreground".
        assert_eq!(
            foreground_check(None, "cn.edu.fzu.fdxypa").map(|(code, _)| code),
            Some("foreground_query_failed")
        );
    }

    #[test]
    fn config_default_has_no_tool_paths_so_first_run_opens_setup() {
        // The Setup page is entered when the config has no tool paths yet; that
        // predicate must stay true for the shipped defaults.
        let config = Config::default();
        assert!(config.emulator.ldconsole_path.trim().is_empty());
        assert!(config.emulator.adb_path.trim().is_empty());
    }

    #[test]
    fn labeled_bounds_match_accessibility_suffix_and_skip_header() {
        let xml = r#"<node content-desc="晚点名签到" bounds="[10,20][30,40]"/><node content-desc="业务&#10;第 4 个标签，共 4 个" bounds="[100,600][300,700]"/>"#;
        assert_eq!(find_labeled_bounds(&page(xml), "晚点名签到", 100), None);
        assert_eq!(
            find_labeled_bounds(&page(xml), "业务", 100),
            Some((200, 650))
        );
        let clickable =
            r#"<node content-desc="晚点名签到" clickable="true" bounds="[10,200][30,240]"/>"#;
        assert_eq!(
            find_clickable_labeled_bounds(&page(clickable), "晚点名签到", 100),
            Some((20, 220))
        );
        let malformed_then_valid = r#"<node content-desc="broken><node content-desc="晚点名签到" clickable="true" bounds="[10,200][30,240]"/>"#;
        assert_eq!(
            find_clickable_labeled_bounds(&page(malformed_then_valid), "晚点名签到", 100),
            Some((20, 220))
        );
    }

    #[test]
    fn app_restart_dialog_outranks_checkin_and_login_markers() {
        let dialog = page(
            r#"
            <node text="晚点名签到"/>
            <node text="签到成功"/>
            <node text="是否重启App"/>
            <node text="当前 App 可能被界面劫持"/>
            <node text="取消"/>
            <node text="重启App"/>
            "#,
        );
        assert_eq!(detect_page_route(&dialog), PageRoute::AppRestartDialog);
        assert_eq!(
            PageRoute::AppRestartDialog.error_code(),
            Some("app_restart_dialog")
        );
        assert_eq!(PageRoute::AppRestartDialog.as_str(), "app_restart_dialog");
    }

    #[test]
    fn login_page_detection_rejects_login_and_home_as_checkin() {
        assert_eq!(
            detect_page_route(&page(r#"<node content-desc="一键登录"/>"#)),
            PageRoute::Login
        );
        assert_eq!(
            detect_page_route(&page(r#"<node content-desc="我的服务"/>"#)),
            PageRoute::Home
        );
        assert_eq!(
            detect_page_route(&page(r#"<node text="晚点名签到"/><node text="无法签到"/>"#)),
            PageRoute::Checkin
        );
        assert_eq!(
            detect_page_route(&page(
                r#"<node text="晚点名签到"/><node text="未签到"/><node text="外勤签到"/><node text="当前位置"/>"#
            )),
            PageRoute::Checkin
        );
        assert_eq!(
            detect_page_route(&page(r#"<node text="我的服务"/><node text="外勤签到"/>"#)),
            PageRoute::Home
        );
        assert_eq!(
            detect_page_route(&page("<hierarchy />")),
            PageRoute::Unknown
        );
    }

    #[test]
    fn page_route_prioritizes_home_and_services_over_checkin_card_status_words() {
        assert_eq!(
            detect_page_route(&page(
                r#"<node text="我的服务"/><node text="晚点名签到"/><node text="已签到"/>"#
            )),
            PageRoute::Home
        );
        assert_eq!(
            detect_page_route(&page(r#"<node text="业务"/><node text="签到成功"/>"#)),
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
            detect_page_route(&page(r#"<node text="点击签到"/>"#)),
            PageRoute::Unknown
        );
        assert_eq!(
            detect_page_route(&page(r#"<node text="已签到"/>"#)),
            PageRoute::Unknown
        );
        assert_eq!(
            detect_page_route(&page(
                r#"<node text="外勤签到"/><node text="未签到"/><node text="当前位置"/>"#
            )),
            PageRoute::Unknown
        );
    }

    #[test]
    fn checkin_mode_is_distinct_from_visual_color() {
        assert_eq!(
            super::detect_checkin_mode(&page(
                r#"<node text="晚点名签到"/><node text="未签到"/><node text="外勤签到"/>"#
            )),
            CheckinMode::FieldWork
        );
        assert_eq!(
            super::detect_checkin_mode(&page(r#"<node text="无法签到"/><node text="点击签到"/>"#)),
            CheckinMode::Ordinary
        );
        assert_eq!(
            super::detect_checkin_mode(&page("<hierarchy />")),
            CheckinMode::Unsupported
        );
    }

    #[test]
    fn success_date_parser_supports_chinese_and_numeric_formats() {
        assert_eq!(
            parse_success_date(r#"<node text="签到成功：2026年10月10日"/>"#),
            chrono::NaiveDate::from_ymd_opt(2026, 10, 10)
        );
        assert_eq!(
            parse_success_date(r#"<node text="日期 2026-10-10"/>"#),
            chrono::NaiveDate::from_ymd_opt(2026, 10, 10)
        );
        assert_eq!(parse_success_date(r#"<node text="没有日期"/>"#), None);
    }

    #[test]
    fn unrelated_update_date_is_not_accepted_as_success_date() {
        let hierarchy = r#"<node text="签到成功"/><node text="上次更新 2026-10-11"/>"#;
        assert_eq!(parse_success_date(hierarchy), None);
    }

    #[test]
    fn success_page_date_row_after_the_date_icon_is_accepted() {
        // Real layout: the calendar icon carries `content-desc="date"` and the
        // value is the next sibling node, so the date is not in a node that
        // itself contains 签到成功.
        let hierarchy = concat!(
            r#"<node content-desc="晚点名签到"/>"#,
            r#"<node text="晚点打卡"/>"#,
            r#"<node text="签到成功"/>"#,
            r#"<node content-desc="date"/>"#,
            r#"<node text="2026-10-17 星期六"/>"#,
            r#"<node text="签到时段：21:00 -23:59"/>"#,
            r#"<node text="已签到："/>"#,
            r#"<node text="2026-10-17 22:37:10"/>"#,
            r#"<node text="签到成功"/>"#
        );
        assert_eq!(
            parse_success_date(hierarchy),
            chrono::NaiveDate::from_ymd_opt(2026, 10, 17)
        );
    }

    #[test]
    fn success_page_split_checkin_timestamp_is_accepted() {
        // `已签到：` and its timestamp are separate nodes; the timestamp alone
        // has no check-in wording of its own.
        let hierarchy = concat!(
            r#"<node content-desc="晚点名签到"/>"#,
            r#"<node text="签到成功"/>"#,
            r#"<node text="已签到："/>"#,
            r#"<node text="2026-10-17 22:37:10"/>"#
        );
        assert_eq!(
            parse_success_date(hierarchy),
            chrono::NaiveDate::from_ymd_opt(2026, 10, 17)
        );
    }

    #[test]
    fn split_date_anchor_does_not_accept_a_conflicting_date() {
        // The anchored date disagrees with another date on the page: fail closed
        // instead of guessing which one is the check-in date.
        let hierarchy = concat!(
            r#"<node content-desc="date"/>"#,
            r#"<node text="2026-10-17 星期六"/>"#,
            r#"<node text="上次更新 2026-10-16"/>"#
        );
        assert_eq!(parse_success_date(hierarchy), None);
    }

    #[test]
    fn success_banner_does_not_anchor_an_unrelated_date() {
        // A result banner must not lend its context to the next node.
        let hierarchy = r#"<node text="签到成功"/><node text="2026-10-11"/>"#;
        assert_eq!(parse_success_date(hierarchy), None);
    }

    #[test]
    fn foreign_package_dates_never_supply_or_veto_the_success_date() {
        // A status-bar or system node with a different package must not add a
        // date (which would trip the all-dates-agree rule) nor anchor one.
        let hierarchy = concat!(
            r#"<node package="com.android.systemui" content-desc="date"/>"#,
            r#"<node package="com.android.systemui" text="2026-10-16 星期五"/>"#,
            r#"<node package="cn.edu.fzu.fdxypa" content-desc="date"/>"#,
            r#"<node package="cn.edu.fzu.fdxypa" text="2026-10-17 星期六"/>"#,
            r#"<node package="cn.edu.fzu.fdxypa" text="已签到："/>"#,
            r#"<node package="cn.edu.fzu.fdxypa" text="2026-10-17 22:37:10"/>"#
        );
        assert_eq!(
            parse_success_date(hierarchy),
            chrono::NaiveDate::from_ymd_opt(2026, 10, 17)
        );
    }

    #[test]
    fn foreign_package_date_alone_is_not_a_success_date() {
        let hierarchy = concat!(
            r#"<node package="cn.edu.fzu.fdxypa" text="签到成功"/>"#,
            r#"<node package="com.android.systemui" content-desc="date"/>"#,
            r#"<node package="com.android.systemui" text="2026-10-17"/>"#
        );
        assert_eq!(parse_success_date(hierarchy), None);
    }

    #[test]
    fn hierarchy_date_is_unknown_when_success_and_unrelated_dates_conflict() {
        let hierarchy = r#"<node text="2026年10月11日"/><node text="签到成功：2026年10月10日"/>"#;
        assert_eq!(parse_success_date(hierarchy), None);
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
            None,
        );
        assert_eq!(
            command,
            r#""C:\Program Files\ZHFD\zhfd-checkin.exe" run --dry-run --instance-index 1"#
        );
    }

    #[test]
    fn task_command_line_keeps_explicit_config_path() {
        // A scheduled task starts a fresh process: without pinning `--config`
        // the task would silently act on whatever root the portable/LocalAppData
        // probe resolves to instead of the config the user actually edited.
        let target = super::InstanceTargetArgs::default();
        let command = build_task_command_line(
            Path::new(r"C:\Program Files\ZHFD\zhfd-checkin.exe"),
            "run --dry-run",
            false,
            &target,
            Some(Path::new(r"C:\User Data\ZHFD\config.toml")),
        );
        assert!(
            command.contains(r#"--config "C:\User Data\ZHFD\config.toml""#),
            "scheduled task dropped the explicit config path: {command}"
        );
    }

    #[test]
    fn task_command_line_is_already_a_complete_task_scheduler_action() {
        // Regression: `schtasks /TR` stores its value verbatim, so the built
        // string must already be the exact action command line. An extra outer
        // pair of quotes made the Task Scheduler read the program name as `""`
        // and fail the launch with 2147942487 (E_INVALIDARG).
        let target = super::InstanceTargetArgs::default();
        let command = build_task_command_line(
            Path::new(r"C:\Program Files\ZHFD\zhfd-checkin.exe"),
            "run --live --confirm",
            true,
            &target,
            Some(Path::new(r"C:\Data\config.toml")),
        );
        // Starts with the quoted executable and never with a doubled quote.
        assert!(command.starts_with(r#""C:\Program Files\ZHFD\zhfd-checkin.exe" "#));
        assert!(
            !command.starts_with("\"\""),
            "action command line must not be wrapped in a second pair of quotes: {command}"
        );
        assert!(command.ends_with("--all-instances"));
        // Exactly four quote characters: the exe pair and the config pair.
        assert_eq!(
            command.matches('"').count(),
            4,
            "unexpected quoting in action command line: {command}"
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
            allow_field_work: false,
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

        // Field work stays denied while the operator has not opted in.
        let mut field_work = request(ButtonState::Ready);
        field_work.checkin_mode = CheckinMode::FieldWork;
        assert!(guarded_tap(&device, field_work).is_err());
        assert!(device.taps.borrow().is_empty());

        // With the opt-in the same request is authorized and taps once.
        let mut field_work_allowed = request(ButtonState::Ready);
        field_work_allowed.checkin_mode = CheckinMode::FieldWork;
        field_work_allowed.allow_field_work = true;
        assert!(guarded_tap(&device, field_work_allowed).unwrap());
        assert_eq!(device.taps.borrow().len(), 1);

        // An unconfirmed business mode is refused even with the opt-in.
        let mut unsupported = request(ButtonState::Ready);
        unsupported.checkin_mode = CheckinMode::Unsupported;
        unsupported.allow_field_work = true;
        assert!(guarded_tap(&device, unsupported).is_err());
        assert_eq!(device.taps.borrow().len(), 1);

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

        // Exactly one tap happened in this test: the explicitly opted-in
        // field-work request. Every denied request above must not have tapped.
        assert_eq!(device.taps.borrow().len(), 1);
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
