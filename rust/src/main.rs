mod adb;
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
use logger::RunLog;
use std::{
    path::PathBuf,
    process::Command,
    thread,
    time::{Duration, Instant},
};

const LDPLAYER_URL: &str = "https://www.ldmnq.com/";
const ZHFD_URL: &str = "https://app.fzu.edu.cn/fd-app/m/index.html";
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser, Debug)]
#[command(name="zhfd-checkin", version=VERSION, about="智汇福大 LDPlayer 自动签到控制器")]
struct Cli {
    #[command(subcommand)]
    command: Option<CommandKind>,
}

#[derive(Subcommand, Debug)]
enum CommandKind {
    Diagnose,
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
    Detect,
    Sync {
        #[arg(long)]
        id: String,
        #[arg(long)]
        confirm: bool,
    },
}
#[derive(Subcommand, Debug)]
enum AppCommand {
    Install {
        #[arg(long)]
        apk: PathBuf,
    },
}
#[derive(Subcommand, Debug)]
enum ConfigCommand {
    Show,
    Open,
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
    Dump,
}
#[derive(Args, Debug)]
struct RunArgs {
    #[arg(long, conflicts_with = "live")]
    dry_run: bool,
    #[arg(long)]
    live: bool,
    #[arg(long, requires = "live")]
    confirm: bool,
}

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

fn run_cli() -> Result<ExitCode> {
    let cli = Cli::parse();
    let store = ConfigStore::load()?;
    match cli.command.unwrap_or(CommandKind::Gui) {
        CommandKind::About => {
            print_about();
            Ok(ExitCode::Ok)
        }
        CommandKind::Gui => {
            gui::launch(store)?;
            Ok(ExitCode::Ok)
        }
        CommandKind::Task { command } => task_command(command),
        CommandKind::Ui { command } => ui_command(&store, command),
        CommandKind::Vision { image } => analyze_image(&image),
        CommandKind::Config { command } => config_command(&store, command),
        CommandKind::Profile { command } => profile_command(&store, command),
        CommandKind::Diagnose => diagnose(&store),
        CommandKind::App { command } => app_command(&store, command),
        CommandKind::Run(args) => run(&store, args),
    }
}

fn analyze_image(path: &PathBuf) -> Result<ExitCode> {
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
    let analysis = vision::classify_button(&image, button);
    println!("{}", serde_json::to_string_pretty(&analysis)?);
    Ok(ExitCode::Ok)
}

fn print_about() {
    println!("zhfd-checkin {}", VERSION);
    println!("Rust Windows 便携式 CLI");
    println!("雷电模拟器官网: {}", LDPLAYER_URL);
    println!("智汇福大官网: {}", ZHFD_URL);
    println!("配置/日志根目录: EXE 所在目录");
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
        ProfileCommand::Detect => {
            let (_, device, info) = runtime(store, false)?;
            print_device(&device.serial, &info);
            match profile::find_profile(info.width, info.height, info.density_dpi) {
                Some(p) => println!("profile={}", p.id),
                None => println!("profile=unknown"),
            };
        }
        ProfileCommand::Sync { id, confirm } => {
            sync_profile(store, &id, confirm)?;
        }
    }
    Ok(ExitCode::Ok)
}

fn sync_profile(store: &ConfigStore, id: &str, confirm: bool) -> Result<ExitCode> {
    if !confirm {
        bail!("同步模拟器配置会修改分辨率/DPI并重启实例，请指定 --confirm");
    }
    let target =
        profile::find_profile_by_id(id).with_context(|| format!("未知 profile: {}", id))?;
    let ld = ldplayer::LdPlayer::discover(&store.config.emulator.ldconsole_path, &store.root)?;
    let index = store.config.emulator.instance_index;
    let instance = ld.instance(index)?;
    let source = ld.executable.parent().map(|parent| {
        parent
            .join("vms")
            .join("config")
            .join(format!("leidian{}.config", index))
    });
    let backup = source.as_ref().filter(|p| p.is_file()).map(|p| {
        p.with_extension(format!(
            "config.bak.{}",
            chrono::Local::now().format("%Y%m%d%H%M%S")
        ))
    });
    if let (Some(source), Some(backup)) = (source.as_ref().filter(|p| p.is_file()), backup.as_ref())
    {
        std::fs::copy(source, backup)
            .with_context(|| format!("无法备份 LDPlayer 配置: {}", source.display()))?;
        println!("已备份: {}", backup.display());
    }
    let resolution = format!("{},{},{}", target.width, target.height, target.density_dpi);
    if let Err(error) = ld.command(&[
        "modify",
        "--index",
        &index.to_string(),
        "--resolution",
        &resolution,
    ]) {
        if let (Some(source), Some(backup)) =
            (source.as_ref().filter(|p| p.is_file()), backup.as_ref())
        {
            let _ = std::fs::copy(backup, source);
        }
        return Err(error).context("同步 LDPlayer 分辨率/DPI失败");
    }
    println!("已写入 profile {}: {}", target.id, resolution);
    if instance.running {
        ld.command(&["quit", "--index", &index.to_string()])?;
        ld.launch_wait(
            index,
            Duration::from_secs(store.config.runtime.startup_timeout_seconds),
        )?;
    }
    println!("同步完成；model/IMEI/账号/定位数据未修改");
    Ok(ExitCode::Ok)
}

fn diagnose(store: &ConfigStore) -> Result<ExitCode> {
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
    let (_, device, info) = runtime(store, false)?;
    print_device(&device.serial, &info);
    if let Some(p) = profile::find_profile(info.width, info.height, info.density_dpi) {
        println!("profile={} calibrated={}", p.id, p.calibrated);
    } else {
        println!("profile=unknown");
    }
    Ok(ExitCode::Ok)
}

fn app_command(store: &ConfigStore, command: AppCommand) -> Result<ExitCode> {
    match command {
        AppCommand::Install { apk } => {
            let (_, device, _) = runtime(store, true)?;
            println!("安装 APK: {}", apk.display());
            println!("{}", device.install_apk(&apk)?);
            let version = device.package_version(&store.config.app.package_name)?;
            println!(
                "安装后包校验: {} {}",
                store.config.app.package_name, version
            );
            let mut saved = store.clone();
            saved.config.app.apk_path = config::path_for_config(&store.root, &apk);
            saved.save()?;
            Ok(ExitCode::Ok)
        }
    }
}

fn task_command(command: TaskCommand) -> Result<ExitCode> {
    match command {
        TaskCommand::Create {
            name,
            live,
            confirm,
        } => {
            if live && !confirm {
                bail!("正式计划任务必须同时指定 --live --confirm");
            }
            let exe = std::env::current_exe().context("无法取得 EXE 路径")?;
            let mode = if live {
                "run --live --confirm"
            } else {
                "run --dry-run"
            };
            let task_run = format!("\"{}\" {}", exe.display(), mode);
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

fn ui_command(store: &ConfigStore, command: UiCommand) -> Result<ExitCode> {
    match command {
        UiCommand::Dump => {
            let (_, device, _) = runtime(store, false)?;
            println!("{}", device.dump_hierarchy()?);
        }
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
    if mode == Mode::Live && !args.confirm {
        bail!("正式模式必须同时指定 --live --confirm")
    }
    let log = RunLog::new(&store.root)?;
    let mut result = RunResult {
        mode: mode.as_str().into(),
        serial: String::new(),
        profile_id: None,
        state_history: vec![],
        clicked: false,
        success: false,
        dry_run_ready: false,
        error: None,
    };
    log.event("start", serde_json::json!({"mode":mode.as_str()}))?;
    let (_, device, info) = match runtime(store, true) {
        Ok(v) => v,
        Err(e) => {
            result.error = Some(e.to_string());
            let _ = log.save_json("result.json", &result);
            println!("运行环境失败: {:#}", e);
            return Ok(ExitCode::DeviceFailure);
        }
    };
    result.serial = device.serial.clone();
    let profile = match profile::find_profile(info.width, info.height, info.density_dpi) {
        Some(p) => p,
        None => {
            result.error = Some("unknown_profile".into());
            log.event(
                "profile_unknown",
                serde_json::json!({"width":info.width,"height":info.height,"dpi":info.density_dpi}),
            )?;
            log.save_json("result.json", &result)?;
            return Ok(ExitCode::VisionFailure);
        }
    };
    result.profile_id = Some(profile.id.to_string());
    if mode == Mode::Live && !profile.calibrated {
        bail!(
            "当前 profile 尚未完成生产标定，正式模式禁止点击: {}",
            profile.id
        )
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
    let _initial_button = locate_or_navigate(&device, profile, &log)?;
    let started = Instant::now();
    let mut unknowns = 0u32;
    let mut ready_frames = 0u32;
    let mut click_deadline: Option<Instant> = None;
    loop {
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
        if click_deadline.is_none()
            && started.elapsed()
                > Duration::from_secs(store.config.runtime.location_timeout_seconds)
        {
            result.error = Some("location_timeout".into());
            break;
        }
        match analysis.state {
            vision::ButtonState::Gray => {
                ready_frames = 0;
                thread::sleep(Duration::from_secs(
                    store.config.runtime.locating_poll_seconds,
                ));
            }
            vision::ButtonState::Locating => {
                ready_frames = 0;
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
                if let Err(policy_error) = domain::authorize_click(
                    mode,
                    args.confirm,
                    profile.calibrated,
                    foreground_matches,
                    policy_window,
                    ready_frames,
                    store.config.runtime.stable_frames,
                ) {
                    result.error = Some(policy_error.to_string());
                    break;
                }
                device.tap(confirm_button.cx, confirm_button.cy)?;
                result.clicked = true;
                log.event(
                    "clicked_ready",
                    serde_json::json!({"x":confirm_button.cx,"y":confirm_button.cy}),
                )?;
                click_deadline = Some(Instant::now());
            }
            vision::ButtonState::Success => unreachable!("success handled before match"),
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
        if let Some(deadline) = click_deadline {
            if deadline.elapsed()
                > Duration::from_secs(store.config.runtime.success_timeout_seconds)
            {
                result.error = Some("success_timeout".into());
                break;
            } else {
                thread::sleep(Duration::from_secs(3));
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
    log.save_json("result.json", &result)?;
    log.event("finish", &result)?;
    println!("结果: {}", serde_json::to_string_pretty(&result)?);
    Ok(code)
}

fn runtime(
    store: &ConfigStore,
    launch: bool,
) -> Result<(ldplayer::LdPlayer, adb::AdbDevice, adb::DeviceInfo)> {
    let ld = ldplayer::LdPlayer::discover(&store.config.emulator.ldconsole_path, &store.root)?;
    let instance = ld.instance(store.config.emulator.instance_index)?;
    if launch && store.config.emulator.auto_launch && !instance.running {
        ld.launch_wait(
            instance.index,
            Duration::from_secs(store.config.runtime.startup_timeout_seconds),
        )?;
    }
    let adb_path = adb::find_adb(&store.config.emulator.adb_path)?;
    let serial = if !store.config.emulator.serial.trim().is_empty() {
        store.config.emulator.serial.clone()
    } else {
        ldplayer::LdPlayer::serial_for_index(instance.index)
    };
    let _ = Command::new(&adb_path).args(["connect", &serial]).output();
    let device = adb::AdbDevice::discover(&adb_path.to_string_lossy(), &serial)?;
    let info = device.info();
    Ok((ld, device, info?))
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
    if let Some(button) = vision::find_colored_button(&image) {
        let analysis = vision::classify_button(&image, button);
        if analysis.state != vision::ButtonState::Unknown {
            return Ok(button);
        }
    }
    let xml = device.dump_hierarchy().unwrap_or_default();
    if let Some((x, y)) = find_entry_bounds(&xml, "晚点名签到") {
        device.tap(x, y)?;
        thread::sleep(Duration::from_secs(3));
    } else {
        device.tap(
            (profile.service_icon_center_ratio.0 * image.width() as f32) as u32,
            (profile.service_icon_center_ratio.1 * image.height() as f32) as u32,
        )?;
        thread::sleep(Duration::from_secs(3));
    }
    let bytes = device.screenshot()?;
    log.save_bytes("navigation_result.png", &bytes)?;
    let image = vision::decode(&bytes)?;
    if let Some(button) = vision::find_colored_button(&image) {
        return Ok(button);
    }
    Ok(vision::fallback_button(&image, profile))
}

fn find_entry_bounds(xml: &str, label: &str) -> Option<(u32, u32)> {
    let marker1 = format!("content-desc=\"{}\"", label);
    let marker2 = format!("text=\"{}\"", label);
    let pos = xml.find(&marker1).or_else(|| xml.find(&marker2))?;
    let tail = &xml[pos..];
    let b = tail.find("bounds=\"")?;
    let value = &tail[b + 8..];
    let end = value.find('"')?;
    let token = &value[..end];
    let nums: Vec<u32> = token
        .split(['[', ']', ','])
        .filter_map(|s| s.parse().ok())
        .collect();
    (nums.len() >= 4).then(|| ((nums[0] + nums[2]) / 2, (nums[1] + nums[3]) / 2))
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
