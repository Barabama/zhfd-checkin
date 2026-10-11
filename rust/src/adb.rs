use crate::profile::Orientation;
use anyhow::{Context, Result, bail};
use std::{
    path::{Path, PathBuf},
    process::Output,
};

#[derive(Debug, Clone)]
pub struct AdbDevice {
    pub adb: PathBuf,
    pub serial: String,
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub model: String,
    pub sdk: String,
    pub width: u32,
    pub height: u32,
    pub density_dpi: u32,
    pub orientation: Orientation,
    pub package: Option<String>,
}

impl AdbDevice {
    pub fn discover(adb_config: &str, serial_config: &str) -> Result<Self> {
        let adb = find_adb(adb_config)?;
        let serial = if !serial_config.trim().is_empty() {
            serial_config.to_string()
        } else {
            let devices = list_devices(&adb)?;
            match devices.as_slice() {
                [one] => one.clone(),
                [] => bail!("没有检测到 ADB 设备，请启动 LDPlayer 或运行 adb connect"),
                many => bail!(
                    "检测到多个 ADB 设备，请在 config.toml 指定 serial: {}",
                    many.join(", ")
                ),
            }
        };
        let device = Self { adb, serial };
        device.ensure_online()?;
        Ok(device)
    }

    pub fn run(&self, args: &[&str]) -> Result<Output> {
        let output = crate::process::command(&self.adb)
            .args(["-s", &self.serial])
            .args(args)
            .output()
            .with_context(|| format!("执行 adb 失败: {}", self.adb.display()))?;
        if !output.status.success() {
            bail!("adb 命令失败({}): {}", output.status, text(&output.stderr));
        }
        Ok(output)
    }

    pub fn shell(&self, args: &[&str]) -> Result<String> {
        let output = crate::process::command(&self.adb)
            .args(["-s", &self.serial, "shell"])
            .args(args)
            .output()
            .with_context(|| format!("执行 adb shell 失败: {}", self.serial))?;
        if !output.status.success() {
            bail!("adb shell 失败: {}", text(&output.stderr));
        }
        Ok(text(&output.stdout))
    }

    pub fn ensure_online(&self) -> Result<()> {
        let state = crate::process::command(&self.adb)
            .args(["-s", &self.serial, "get-state"])
            .output()
            .with_context(|| format!("检查 ADB 设备失败: {}", self.serial))?;
        if !state.status.success() || text(&state.stdout).trim() != "device" {
            bail!(
                "ADB 设备不可用: {} ({})",
                self.serial,
                text(&state.stderr).trim()
            );
        }
        Ok(())
    }

    pub fn info(&self) -> Result<DeviceInfo> {
        let size = self.shell(&["wm", "size"])?;
        let density = self.shell(&["wm", "density"])?;
        let model = self
            .shell(&["getprop", "ro.product.model"])?
            .trim()
            .to_string();
        let sdk = self
            .shell(&["getprop", "ro.build.version.sdk"])?
            .trim()
            .to_string();
        let (width, height) = parse_size(&size).context("无法解析 wm size")?;
        let dpi = parse_number_after(&density, "density").unwrap_or(0);
        Ok(DeviceInfo {
            model,
            sdk,
            width,
            height,
            density_dpi: dpi,
            orientation: Orientation::from_dimensions(width, height),
            package: self.current_package().ok(),
        })
    }

    pub fn current_package(&self) -> Result<String> {
        let text = self.shell(&["dumpsys", "window"])?;
        for line in text.lines() {
            if (line.contains("mCurrentFocus=") || line.contains("mFocusedApp="))
                && let Some(pkg) = parse_package(line)
            {
                return Ok(pkg);
            }
        }
        bail!("无法读取当前前台包名")
    }

    pub fn screenshot(&self) -> Result<Vec<u8>> {
        let output = crate::process::command(&self.adb)
            .args(["-s", &self.serial, "exec-out", "screencap", "-p"])
            .output()?;
        if !output.status.success() {
            bail!("截图失败: {}", text(&output.stderr));
        }
        if output.stdout.is_empty() {
            bail!(
                "截图返回空内容，设备可能 offline 或正在重启: {}",
                self.serial
            );
        }
        Ok(output.stdout)
    }

    pub fn launch_package(&self, package: &str, activity: &str) -> Result<()> {
        self.shell(&["am", "start", "-n", &format!("{}/{}", package, activity)])?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn force_stop(&self, package: &str) -> Result<()> {
        self.shell(&["am", "force-stop", package])?;
        Ok(())
    }

    pub fn tap(&self, x: u32, y: u32) -> Result<()> {
        self.shell(&["input", "tap", &x.to_string(), &y.to_string()])?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn swipe(&self, x1: u32, y1: u32, x2: u32, y2: u32, duration_ms: u32) -> Result<()> {
        self.shell(&[
            "input",
            "swipe",
            &x1.to_string(),
            &y1.to_string(),
            &x2.to_string(),
            &y2.to_string(),
            &duration_ms.to_string(),
        ])?;
        Ok(())
    }

    pub fn dump_hierarchy(&self) -> Result<String> {
        self.shell(&["uiautomator", "dump", "/sdcard/window.xml"])?;
        self.shell(&["cat", "/sdcard/window.xml"])
    }

    pub fn install_apk(&self, apk: &Path) -> Result<String> {
        if !apk.is_file() {
            bail!("APK 不存在: {}", apk.display());
        }
        let path = apk.to_string_lossy().to_string();
        let output = self.run(&["install", "-r", &path])?;
        Ok(text(&output.stdout))
    }

    pub fn package_version(&self, package: &str) -> Result<String> {
        let output = self.shell(&["dumpsys", "package", package])?;
        for line in output.lines() {
            if line.trim_start().starts_with("versionName=") {
                return Ok(line.trim().to_string());
            }
        }
        bail!("未找到已安装包: {}", package)
    }
}

pub fn find_adb(configured: &str) -> Result<PathBuf> {
    find_adb_for_ldplayer(configured, None, None)
}

pub fn find_adb_for_ldplayer(
    configured: &str,
    ldconsole: Option<&Path>,
    config_root: Option<&Path>,
) -> Result<PathBuf> {
    let report = discover_adb(configured, ldconsole, config_root);
    report
        .selected_path()
        .ok_or_else(|| anyhow::anyhow!(report.summary()))
}

pub fn discover_adb(
    configured: &str,
    ldconsole: Option<&Path>,
    config_root: Option<&Path>,
) -> crate::discovery::DiscoveryReport {
    let mut candidates: Vec<(PathBuf, &'static str)> = Vec::new();
    if !configured.trim().is_empty() {
        let path = PathBuf::from(configured);
        let path = if path.is_absolute() {
            path
        } else if let Some(root) = config_root {
            root.join(path)
        } else {
            path
        };
        candidates.push((path, "config"));
    }
    if let Some(ldconsole) = ldconsole
        && let Some(parent) = ldconsole.parent()
    {
        for relative in [
            "adb.exe",
            "platform-tools/adb.exe",
            "LDPlayer/adb.exe",
            "LDPlayer/LDPlayer9/adb.exe",
            "LDPlayer/LDPlayer14/adb.exe",
            "LDPlayer9/adb.exe",
            "LDPlayer14/adb.exe",
        ] {
            candidates.push((parent.join(relative), "ldplayer_installation"));
        }
    }
    if let Ok(value) = std::env::var("ANDROID_ADB_PATH")
        && !value.trim().is_empty()
    {
        candidates.push((PathBuf::from(value), "environment"));
    }
    for variable in ["ANDROID_ADB_DIR", "ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Ok(value) = std::env::var(variable)
            && !value.trim().is_empty()
        {
            let root = PathBuf::from(value);
            candidates.push((root.join("platform-tools/adb.exe"), "environment"));
            candidates.push((root.join("adb.exe"), "environment"));
        }
    }
    if cfg!(windows) {
        if let Ok(output) = crate::process::command("where.exe").arg("adb.exe").output()
            && output.status.success()
        {
            for line in String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter(|line| !line.trim().is_empty())
            {
                candidates.push((PathBuf::from(line.trim()), "path"));
            }
        }
    } else if let Ok(output) = crate::process::command("which").arg("adb").output()
        && output.status.success()
    {
        for line in String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| !line.trim().is_empty())
        {
            candidates.push((PathBuf::from(line.trim()), "path"));
        }
    }
    crate::discovery::DiscoveryReport::from_candidates("Android Debug Bridge adb.exe", candidates)
}

pub fn list_devices(adb: &Path) -> Result<Vec<String>> {
    let output = crate::process::command(adb).arg("devices").output()?;
    if !output.status.success() {
        bail!("adb devices 失败: {}", text(&output.stderr));
    }
    Ok(text(&output.stdout)
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let serial = parts.next()?;
            let state = parts.next()?;
            (state == "device").then(|| serial.to_string())
        })
        .collect())
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_string()
}
fn parse_size(text: &str) -> Option<(u32, u32)> {
    text.lines().rev().find_map(|line| {
        let token = line.split_whitespace().last()?;
        let (w, h) = token.split_once('x')?;
        Some((w.parse().ok()?, h.parse().ok()?))
    })
}
fn parse_number_after(text: &str, word: &str) -> Option<u32> {
    text.lines().find_map(|line| {
        line.find(word).and_then(|i| {
            line[i + word.len()..]
                .split_whitespace()
                .find_map(|v| v.parse().ok())
        })
    })
}
fn parse_package(line: &str) -> Option<String> {
    let after = line.split_whitespace().find(|x| x.contains("/"))?;
    let token = after.trim_matches(|c| c == '}' || c == '{' || c == ',');
    let pkg = token.split('/').next()?;
    (pkg.contains('.')).then(|| pkg.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configured_adb_path_wins_over_ldplayer_sibling_candidate() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("zhfd-adb-priority-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let configured = root.join("configured-adb.exe");
        let ldconsole = root.join("ldconsole.exe");
        let sibling = root.join("adb.exe");
        std::fs::write(&configured, b"configured").unwrap();
        std::fs::write(&ldconsole, b"ld").unwrap();
        std::fs::write(&sibling, b"sibling").unwrap();
        let report = discover_adb(
            configured.to_string_lossy().as_ref(),
            Some(&ldconsole),
            Some(&root),
        );
        assert_eq!(report.selected_path(), Some(configured));
        assert_eq!(report.selected().unwrap().source, "config");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sibling_adb_is_selected_when_config_candidate_is_missing() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("zhfd-adb-sibling-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let ldconsole = root.join("ldconsole.exe");
        let sibling = root.join("adb.exe");
        std::fs::write(&ldconsole, b"ld").unwrap();
        std::fs::write(&sibling, b"adb").unwrap();
        let report = discover_adb("missing/adb.exe", Some(&ldconsole), Some(&root));
        assert_eq!(report.selected_path(), Some(sibling));
        assert_eq!(report.selected().unwrap().source, "ldplayer_installation");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn explicit_adb_candidate_wins_over_ldplayer_sibling() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("zhfd-adb-config-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let explicit = root.join("explicit-adb.exe");
        let ldconsole = root.join("ldconsole.exe");
        let sibling = root.join("adb.exe");
        std::fs::write(&explicit, b"explicit").unwrap();
        std::fs::write(&ldconsole, b"ldconsole").unwrap();
        std::fs::write(&sibling, b"sibling").unwrap();
        let report = discover_adb(
            explicit.to_string_lossy().as_ref(),
            Some(&ldconsole),
            Some(&root),
        );
        assert_eq!(report.selected_path(), Some(explicit));
        assert_eq!(report.selected().unwrap().source, "config");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ldplayer_sibling_adb_is_fallback_when_config_path_is_missing() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("zhfd-adb-sibling-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let ldconsole = root.join("ldconsole.exe");
        let sibling = root.join("adb.exe");
        std::fs::write(&ldconsole, b"ldconsole").unwrap();
        std::fs::write(&sibling, b"adb").unwrap();
        let report = discover_adb("missing/adb.exe", Some(&ldconsole), Some(&root));
        assert_eq!(report.selected_path(), Some(sibling));
        assert_eq!(report.selected().unwrap().source, "ldplayer_installation");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn parses_sizes() {
        assert_eq!(parse_size("Physical size: 900x1600\n"), Some((900, 1600)));
    }
    #[test]
    fn parses_package() {
        assert_eq!(
            parse_package("mCurrentFocus=Window{u0 cn.edu.fzu.fdxypa/cn.edu.fzu.Main}"),
            Some("cn.edu.fzu.fdxypa".into())
        );
    }
}
