use crate::profile::Orientation;
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    process::Output,
};

#[derive(Debug, Clone)]
pub struct AdbDevice {
    pub adb: PathBuf,
    pub serial: String,
}

#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HierarchyStatus {
    DumpSucceeded,
    DumpFailed,
    DumpEmptyOrTruncated,
}

impl HierarchyStatus {
    /// Stable machine label persisted in events and in the run result
    /// (requirement FR-011). `DumpSucceeded` is reported separately because a
    /// successful dump can still fail to identify the page
    /// (`route_markers_missing`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DumpSucceeded => "dump_succeeded",
            Self::DumpFailed => "dump_failed",
            Self::DumpEmptyOrTruncated => "dump_empty_or_truncated",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct HierarchyObservation {
    pub stage: String,
    pub elapsed_ms: u128,
    pub status: HierarchyStatus,
    pub bytes: usize,
    pub utf8_valid: bool,
    pub exit_code: Option<i32>,
    pub stderr_summary: Option<String>,
    pub marker_summary: Option<String>,
    /// Distinct `package` attributes in document order. A package other than
    /// the target app means another window is on screen, which is what makes a
    /// popup or advertisement diagnosable after the fact.
    pub packages: Vec<String>,
    /// `package` of the first node, i.e. the package that owns the dumped
    /// window. `None` when the dump omits the attribute.
    pub root_package: Option<String>,
    /// Number of `<node` elements in the dump. A page that renders normally
    /// carries far more than an overlay card does.
    pub node_count: usize,
    #[serde(skip)]
    pub xml: Option<String>,
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

    /// The raw `mCurrentFocus=`/`mFocusedApp=` line, length-limited.
    ///
    /// `current_package` only yields a package name, which cannot distinguish
    /// "the app's own page" from "a dialog rendered inside the app's window
    /// stack". Keeping the raw line makes an overlay diagnosable instead of
    /// merely suspected.
    pub fn focused_window(&self) -> Result<String> {
        let text = self.shell(&["dumpsys", "window"])?;
        for line in text.lines() {
            if line.contains("mCurrentFocus=") || line.contains("mFocusedApp=") {
                return Ok(limit_error(line));
            }
        }
        bail!("无法读取当前前台窗口")
    }

    /// The package owning the currently focused window, if the window dump
    /// exposes one. This is stronger overlay evidence than an arbitrary
    /// descendant package in the accessibility hierarchy.
    #[expect(dead_code, reason = "used by focused-window diagnostics tests")]
    pub fn focused_package(&self) -> Result<String> {
        let text = self.shell(&["dumpsys", "window"])?;
        for line in text.lines() {
            if (line.contains("mCurrentFocus=") || line.contains("mFocusedApp="))
                && let Some(package) = parse_package(line)
            {
                return Ok(package);
            }
        }
        bail!("无法读取当前前台窗口包名")
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

    #[expect(
        dead_code,
        reason = "reserved for a future bounded recovery path; never used to restart an unknown page"
    )]
    pub fn force_stop(&self, package: &str) -> Result<()> {
        self.shell(&["am", "force-stop", package])?;
        Ok(())
    }

    pub fn tap(&self, x: u32, y: u32) -> Result<()> {
        self.shell(&["input", "tap", &x.to_string(), &y.to_string()])?;
        Ok(())
    }

    pub fn back(&self) -> Result<()> {
        self.shell(&["input", "keyevent", "4"])?;
        Ok(())
    }

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
        let observation = self.dump_hierarchy_observation("direct")?;
        observation.xml.context("hierarchy dump 没有 XML 内容")
    }

    pub fn dump_hierarchy_observation(&self, stage: &str) -> Result<HierarchyObservation> {
        let started = std::time::Instant::now();
        let dump = crate::process::command(&self.adb)
            .args([
                "-s",
                &self.serial,
                "shell",
                "uiautomator",
                "dump",
                "/sdcard/window.xml",
            ])
            .output()
            .with_context(|| format!("执行 hierarchy dump 失败: {}", self.serial))?;
        if !dump.status.success() {
            return Ok(HierarchyObservation {
                stage: stage.into(),
                elapsed_ms: started.elapsed().as_millis(),
                status: HierarchyStatus::DumpFailed,
                bytes: 0,
                utf8_valid: false,
                exit_code: dump.status.code(),
                stderr_summary: Some(limit_error(&text(&dump.stderr))),
                marker_summary: None,
                packages: Vec::new(),
                root_package: None,
                node_count: 0,
                xml: None,
            });
        }
        let cat = crate::process::command(&self.adb)
            .args(["-s", &self.serial, "shell", "cat", "/sdcard/window.xml"])
            .output()
            .with_context(|| format!("读取 hierarchy XML 失败: {}", self.serial))?;
        let utf8 = String::from_utf8(cat.stdout.clone());
        let utf8_valid = utf8.is_ok();
        let xml = utf8.unwrap_or_else(|_| String::from_utf8_lossy(&cat.stdout).into_owned());
        let valid_shape = xml.contains("<hierarchy") && xml.contains("<node");
        let status = if !cat.status.success() {
            HierarchyStatus::DumpFailed
        } else if xml.trim().is_empty() || !valid_shape {
            HierarchyStatus::DumpEmptyOrTruncated
        } else {
            HierarchyStatus::DumpSucceeded
        };
        let readable = matches!(status, HierarchyStatus::DumpSucceeded);
        let packages = if readable {
            observed_packages(&xml)
        } else {
            Vec::new()
        };
        Ok(HierarchyObservation {
            stage: stage.into(),
            elapsed_ms: started.elapsed().as_millis(),
            status: status.clone(),
            bytes: cat.stdout.len(),
            utf8_valid,
            exit_code: cat.status.code(),
            stderr_summary: (!cat.status.success()).then(|| limit_error(&text(&cat.stderr))),
            marker_summary: readable.then(|| marker_summary(&xml)),
            root_package: packages.first().cloned(),
            node_count: if readable {
                xml.matches("<node").count()
            } else {
                0
            },
            packages,
            xml: readable.then_some(xml),
        })
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

/// Truncate an error message for the persisted observation.
///
/// Truncation must land on a character boundary: slicing a UTF-8 string at a
/// raw byte index panics, and a panic here would turn a recoverable ADB error
/// into a crash inside the error-reporting path.
fn limit_error(value: &str) -> String {
    const LIMIT: usize = 512;
    let trimmed = value.trim();
    if trimmed.len() <= LIMIT {
        return trimmed.to_string();
    }
    let end = trimmed
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= LIMIT)
        .last()
        .unwrap_or(0);
    format!("{}…", &trimmed[..end])
}

fn marker_summary(xml: &str) -> String {
    [
        "晚点名签到",
        "点击签到",
        "定位中",
        "签到成功",
        "我的服务",
        "业务",
    ]
    .iter()
    .filter(|marker| xml.contains(**marker))
    .copied()
    .collect::<Vec<_>>()
    .join(",")
}

/// Distinct `package` attributes of a hierarchy dump, in document order.
///
/// Only used for diagnostics: it answers "whose window is this?" without
/// needing another ADB round trip.
fn observed_packages(xml: &str) -> Vec<String> {
    let mut packages: Vec<String> = Vec::new();
    for chunk in xml.split("<node").skip(1) {
        let Some(end) = chunk.find('>') else { continue };
        let tag = &chunk[..end];
        let Some(value) = attribute_value(tag, "package") else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        if !packages.iter().any(|existing| existing == value) {
            packages.push(value.to_string());
        }
    }
    packages
}

/// Value of `name="..."` inside a `<node ...>` tag body.
///
/// The preceding character must be whitespace (or the start of the body) so an
/// attribute name cannot be matched inside another attribute's value.
fn attribute_value<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let prefix = format!("{name}=\"");
    let mut cursor = 0usize;
    while let Some(relative) = tag[cursor..].find(&prefix) {
        let start = cursor + relative;
        let at_boundary = start == 0
            || tag[..start]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        let value_start = start + prefix.len();
        let length = tag[value_start..].find('"')?;
        if at_boundary {
            return Some(&tag[value_start..value_start + length]);
        }
        cursor = value_start + length;
    }
    None
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
/// Extract a package name from the raw `focused_window` line without issuing a
/// second window dump. Keeping this pure lets callers bind focus and hierarchy
/// evidence to the same observation round.
pub fn package_from_focused_window(line: &str) -> Option<String> {
    parse_package(line)
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
    fn observed_packages_reports_foreign_windows_in_document_order() {
        // A real dump names the owning package on every node. The first entry is
        // the window root, which is what identifies a blocking overlay.
        let xml = r#"<hierarchy><node index="0" package="com.example.ad" text="" bounds="[0,0][900,1600]"><node index="0" package="com.example.ad" text="关闭" bounds="[10,10][20,20]"/></node></hierarchy>"#;
        assert_eq!(
            observed_packages(xml),
            vec!["com.example.ad".to_string()],
            "重复包名必须去重且保持文档顺序"
        );

        let mixed = r#"<hierarchy><node package="cn.edu.fzu.fdxypa" text="晚点名签到"/><node package="android" text=""/></hierarchy>"#;
        assert_eq!(
            observed_packages(mixed),
            vec!["cn.edu.fzu.fdxypa".to_string(), "android".to_string()]
        );
    }

    #[test]
    fn observed_packages_tolerates_missing_and_empty_attributes() {
        // Fixtures and partially written dumps may omit the attribute entirely.
        assert!(observed_packages("<hierarchy><node text=\"x\"/></hierarchy>").is_empty());
        assert!(observed_packages(r#"<node package="" text="x"/>"#).is_empty());
    }

    #[test]
    fn attribute_value_requires_a_leading_boundary() {
        // `mypackage="..."` must not satisfy a lookup for `package`.
        let tag = r#" mypackage="decoy" package="real" bounds="[0,0][1,1]""#;
        assert_eq!(attribute_value(tag, "package"), Some("real"));
        assert_eq!(attribute_value(tag, "bounds"), Some("[0,0][1,1]"));
        assert_eq!(attribute_value(tag, "clickable"), None);
        // A value that itself contains `package="` must not win.
        let nested = r#" content-desc="x package=&quot;decoy&quot;" package="real""#;
        assert_eq!(attribute_value(nested, "package"), Some("real"));
    }

    #[test]
    fn hierarchy_status_labels_are_distinct_and_stable() {
        let labels = [
            HierarchyStatus::DumpSucceeded.as_str(),
            HierarchyStatus::DumpFailed.as_str(),
            HierarchyStatus::DumpEmptyOrTruncated.as_str(),
        ];
        let unique: std::collections::HashSet<_> = labels.iter().collect();
        assert_eq!(unique.len(), labels.len(), "层级状态标签必须互不相同");
        assert_eq!(HierarchyStatus::DumpFailed.as_str(), "dump_failed");
        assert_eq!(
            HierarchyStatus::DumpEmptyOrTruncated.as_str(),
            "dump_empty_or_truncated"
        );
        // `route_markers_missing` is the fourth FR-011 outcome and is produced
        // when a successful dump carries no usable page identity.
        assert!(!labels.contains(&"route_markers_missing"));
    }

    #[test]
    fn limit_error_truncates_on_a_character_boundary() {
        // A long run of multi-byte characters previously panicked when the
        // 512-byte cut landed inside a character.
        let long = "错".repeat(400);
        let limited = limit_error(&long);
        assert!(limited.ends_with('…'));
        assert!(limited.len() <= 512 + '…'.len_utf8());
        assert_eq!(limit_error("短错误"), "短错误");
    }

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
