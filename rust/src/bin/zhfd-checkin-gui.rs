#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    let result = zhfd_checkin::launch_gui();
    if let Err(error) = result {
        write_startup_error(&format!("{error:#}\n"));
    }
}

fn write_startup_error(message: &str) {
    if let Ok(exe) = std::env::current_exe()
        && let Some(root) = exe.parent()
    {
        let _ = std::fs::write(root.join("gui-startup-error.log"), message);
    }
}
