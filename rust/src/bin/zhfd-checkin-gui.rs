#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    if let Err(error) = zhfd_checkin::launch_gui() {
        let message = format!("{error:#}");
        let startup_log = write_startup_error(&message);
        show_startup_error(&message, startup_log.as_deref());
        std::process::exit(1);
    }
}

fn write_startup_error(message: &str) -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let root = exe.parent()?;
    let log_path = root.join("gui-startup-error.log");
    let config_path = root.join("config.toml");
    let contents = format!(
        "GUI 启动失败\n配置文件: {}\n错误: {}\n",
        config_path.display(),
        message
    );
    std::fs::write(&log_path, contents).ok()?;
    Some(log_path)
}

#[cfg(windows)]
fn show_startup_error(error: &str, log_path: Option<&std::path::Path>) {
    let exe = std::env::current_exe().ok();
    let config_path = exe
        .as_deref()
        .and_then(std::path::Path::parent)
        .map(|root| root.join("config.toml"));
    let mut message = format!(
        "智汇福大自动签到无法启动。\n\n错误信息：{error}\n\n不会自动覆盖您的配置。请备份 config.toml 后修复 TOML 格式，或暂时将该文件改名再重启。"
    );
    if let Some(path) = config_path {
        message.push_str(&format!("\n\n配置文件：{}", path.display()));
    }
    if let Some(path) = log_path {
        message.push_str(&format!("\n\n诊断日志：{}", path.display()));
    } else {
        message.push_str("\n\n启动日志写入失败，请检查程序目录的写入权限。");
    }
    let _ = rfd::MessageDialog::new()
        .set_title("签到助手启动失败")
        .set_description(message)
        .set_level(rfd::MessageLevel::Error)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

#[cfg(not(windows))]
fn show_startup_error(error: &str, log_path: Option<&std::path::Path>) {
    eprintln!("智汇福大自动签到无法启动: {error}");
    if let Some(path) = log_path {
        eprintln!("诊断日志: {}", path.display());
    }
}
