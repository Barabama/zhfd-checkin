use anyhow::{Context, Result, bail};
use std::{
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

#[derive(Debug, Clone)]
pub struct LdPlayer {
    pub executable: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Instance {
    pub index: u32,
    pub name: String,
    pub running: bool,
    /// Raw column 6 from `ldconsole list2`; do not assume this is the TCP ADB serial port.
    pub ldplayer_reported_port: Option<u16>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub dpi: Option<u32>,
}

impl LdPlayer {
    pub fn discover(configured: &str, root: &Path) -> Result<Self> {
        let report = Self::discover_report(configured, root);
        let executable = report
            .selected_path()
            .ok_or_else(|| anyhow::anyhow!(report.summary()))?;
        Ok(Self { executable })
    }

    /// Candidate priority: config, portable application root, environment,
    /// PATH, registry installation roots, then conventional Windows roots.
    pub fn discover_report(configured: &str, root: &Path) -> crate::discovery::DiscoveryReport {
        let mut candidates: Vec<(PathBuf, &'static str)> = Vec::new();
        if !configured.trim().is_empty() {
            let path = resolve_configured_path(configured, root);
            candidates.push((as_ldconsole_path(path), "config"));
        }
        candidates.push((root.join("ldconsole.exe"), "application_directory"));
        for variable in ["LDPLAYER_PATH", "LDPLAYER_HOME"] {
            if let Ok(value) = std::env::var(variable)
                && !value.trim().is_empty()
            {
                candidates.push((as_ldconsole_path(PathBuf::from(value)), "environment"));
            }
        }
        if cfg!(windows)
            && let Ok(output) = crate::process::command("where.exe")
                .arg("ldconsole.exe")
                .output()
            && output.status.success()
        {
            candidates.extend(
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .map(|line| (PathBuf::from(line.trim()), "path")),
            );
        }
        for install_root in registry_install_locations() {
            for relative in [
                "ldconsole.exe",
                "LDPlayer/ldconsole.exe",
                "LDPlayer/LDPlayer9/ldconsole.exe",
                "LDPlayer/LDPlayer14/ldconsole.exe",
                "LDPlayer9/ldconsole.exe",
                "LDPlayer14/ldconsole.exe",
                "leidian/LDPlayer9/ldconsole.exe",
                "leidian/LDPlayer14/ldconsole.exe",
            ] {
                candidates.push((install_root.join(relative), "registry_installation"));
            }
        }
        for variable in [
            "ProgramFiles",
            "ProgramFiles(x86)",
            "LOCALAPPDATA",
            "ProgramData",
        ] {
            if let Ok(base) = std::env::var(variable)
                && !base.trim().is_empty()
            {
                for relative in [
                    "LDPlayer/ldconsole.exe",
                    "LDPlayer/LDPlayer9/ldconsole.exe",
                    "LDPlayer/LDPlayer14/ldconsole.exe",
                    "leidian/LDPlayer9/ldconsole.exe",
                    "leidian/LDPlayer14/ldconsole.exe",
                ] {
                    candidates.push((
                        PathBuf::from(&base).join(relative),
                        "conventional_installation",
                    ));
                }
            }
        }
        crate::discovery::DiscoveryReport::from_candidates("LDPlayer ldconsole.exe", candidates)
    }

    pub fn command(&self, args: &[&str]) -> Result<String> {
        let output = crate::process::command(&self.executable)
            .args(args)
            .output()
            .with_context(|| format!("执行 LDPlayer 失败: {}", self.executable.display()))?;
        if !output.status.success() {
            bail!(
                "LDPlayer 命令失败: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    pub fn instances(&self) -> Result<Vec<Instance>> {
        let text = self.command(&["list2"])?;
        Ok(text.lines().filter_map(parse_instance).collect())
    }

    pub fn instance(&self, index: u32) -> Result<Instance> {
        self.instances()?
            .into_iter()
            .find(|i| i.index == index)
            .with_context(|| format!("LDPlayer 实例不存在: {}", index))
    }

    pub fn wait_for_stopped(&self, index: u32, timeout: Duration) -> Result<()> {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            if !self.instance(index)?.running {
                return Ok(());
            }
            thread::sleep(Duration::from_secs(1));
        }
        bail!("LDPlayer 实例停止超时: {}", index)
    }

    pub fn launch_wait(&self, index: u32, timeout: Duration) -> Result<()> {
        // After `quit`, `isrunning` can briefly return stale output. Check the
        // parsed instance state and issue launch only while it is stopped.
        if !self.instance(index)?.running {
            self.command(&["launch", "--index", &index.to_string()])?;
        }
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            if self.instance(index)?.running {
                return Ok(());
            }
            thread::sleep(Duration::from_secs(2));
        }
        bail!("LDPlayer 实例启动超时: {}", index)
    }

    /// TCP ADB serial used by `adb connect` for LDPlayer instance indexes.
    /// This is intentionally distinct from the raw port value reported by list2.
    pub fn tcp_serial_for_index(index: u32) -> Option<String> {
        let port = index.checked_mul(2)?.checked_add(5555)?;
        u16::try_from(port)
            .ok()
            .map(|port| format!("127.0.0.1:{port}"))
    }
}

fn resolve_configured_path(configured: &str, root: &Path) -> PathBuf {
    let path = PathBuf::from(configured);
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

fn as_ldconsole_path(path: PathBuf) -> PathBuf {
    if path.is_dir() {
        path.join("ldconsole.exe")
    } else {
        path
    }
}

fn registry_install_locations() -> Vec<PathBuf> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let mut result = Vec::new();
    for root in [
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall",
        r"HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall",
        r"HKLM\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    ] {
        if let Ok(output) = crate::process::command("reg")
            .args(["query", root, "/s", "/v", "DisplayName"])
            .output()
            && output.status.success()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            for uninstall_key in parse_uninstall_keys(&text, "ldplayer") {
                if let Ok(install_output) = crate::process::command("reg")
                    .args(["query", &uninstall_key, "/v", "InstallLocation"])
                    .output()
                    && install_output.status.success()
                {
                    result.extend(parse_registry_values(
                        &String::from_utf8_lossy(&install_output.stdout),
                        &["InstallLocation"],
                    ));
                }
            }
        }
    }
    for key in [
        r"HKCU\Software\leidian\LDPlayer",
        r"HKLM\Software\leidian\LDPlayer",
        r"HKLM\Software\WOW6432Node\leidian\LDPlayer",
        r"HKCU\Software\leidian\LDPlayer9",
        r"HKLM\Software\leidian\LDPlayer9",
        r"HKLM\Software\WOW6432Node\leidian\LDPlayer9",
    ] {
        if let Ok(output) = crate::process::command("reg")
            .args(["query", key, "/s"])
            .output()
            && output.status.success()
        {
            result.extend(parse_registry_values(
                &String::from_utf8_lossy(&output.stdout),
                &["InstallDir", "InstallLocation", "Path"],
            ));
        }
    }
    result
}

fn parse_uninstall_keys(text: &str, product_name: &str) -> Vec<String> {
    let mut matching_keys = Vec::new();
    let mut current_key: Option<&str> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.to_ascii_uppercase().starts_with("HKEY_") {
            current_key = Some(trimmed);
            continue;
        }
        if let Some((name, value)) = parse_registry_value_line(trimmed)
            && name.eq_ignore_ascii_case("DisplayName")
            && value
                .to_ascii_lowercase()
                .contains(&product_name.to_ascii_lowercase())
            && let Some(key) = current_key
        {
            matching_keys.push(key.to_string());
        }
    }
    matching_keys
}

fn parse_registry_values(text: &str, names: &[&str]) -> Vec<PathBuf> {
    text.lines()
        .filter_map(|line| parse_registry_value_line(line.trim()))
        .filter(|(name, value)| {
            !value.is_empty()
                && names
                    .iter()
                    .any(|candidate| name.eq_ignore_ascii_case(candidate))
        })
        .map(|(_, value)| expand_registry_path(&value))
        .collect()
}

fn parse_registry_value_line(line: &str) -> Option<(String, String)> {
    let mut fields = line.split_whitespace();
    let name = fields.next()?;
    let value_type = fields.next()?;
    if !matches!(value_type, "REG_SZ" | "REG_EXPAND_SZ") {
        return None;
    }
    Some((name.to_string(), fields.collect::<Vec<_>>().join(" ")))
}

fn expand_registry_path(value: &str) -> PathBuf {
    let mut expanded = String::new();
    let mut remainder = value;
    while let Some(start) = remainder.find('%') {
        expanded.push_str(&remainder[..start]);
        let after_start = &remainder[start + 1..];
        let Some(end) = after_start.find('%') else {
            expanded.push_str(&remainder[start..]);
            remainder = "";
            break;
        };
        let variable = &after_start[..end];
        if let Ok(replacement) = std::env::var(variable) {
            expanded.push_str(&replacement);
        } else {
            expanded.push('%');
            expanded.push_str(variable);
            expanded.push('%');
        }
        remainder = &after_start[end + 1..];
    }
    expanded.push_str(remainder);
    PathBuf::from(expanded)
}

fn parse_instance(line: &str) -> Option<Instance> {
    let fields: Vec<_> = line.split(',').map(str::trim).collect();
    if fields.len() < 7 {
        return None;
    }
    let index = fields[0].parse().ok()?;
    let running = match fields.get(4).copied()? {
        "1" | "true" => true,
        "0" | "false" => false,
        _ => return None,
    };
    Some(Instance {
        index,
        name: fields.get(1).unwrap_or(&"").to_string(),
        running,
        ldplayer_reported_port: fields.get(6).and_then(|v| v.parse().ok()),
        width: fields.get(7).and_then(|v| v.parse().ok()),
        height: fields.get(8).and_then(|v| v.parse().ok()),
        dpi: fields.get(9).and_then(|v| v.parse().ok()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn relative_configured_ldconsole_is_resolved_against_portable_root() {
        let root = std::env::temp_dir().join(format!(
            "zhfd-ld-relative-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("tools")).unwrap();
        let executable = root.join("tools/ldconsole.exe");
        std::fs::write(&executable, b"fixture").unwrap();
        let report = LdPlayer::discover_report("tools/ldconsole.exe", &root);
        assert_eq!(report.selected_path(), Some(executable));
        assert_eq!(report.selected().unwrap().source, "config");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_matching_uninstall_keys_fixture() {
        let fixture = r#"
HKEY_LOCAL_MACHINE\Software\Vendor\Uninstall\{GAME}
    DisplayName    REG_SZ    Unrelated Game
HKEY_LOCAL_MACHINE\Software\Vendor\Uninstall\{LDPLAYER}
    DisplayName    REG_SZ    LDPlayer 14
"#;
        assert_eq!(
            parse_uninstall_keys(fixture, "ldplayer"),
            vec![r"HKEY_LOCAL_MACHINE\Software\Vendor\Uninstall\{LDPLAYER}".to_string()]
        );
    }

    #[test]
    fn parses_registry_text_fixture_and_ignores_unrelated_values() {
        let fixture = r#"
InstallLocation    REG_SZ    C:\Program Files\LDPlayer14
InstallDir         REG_EXPAND_SZ    %ProgramFiles%\LDPlayer14
DisplayName        REG_SZ    LDPlayer
OtherPath          REG_SZ    C:\Other
"#;
        let values = parse_registry_values(fixture, &["InstallLocation", "InstallDir", "Path"]);
        assert_eq!(values.len(), 2);
        assert_eq!(values[0], PathBuf::from(r"C:\Program Files\LDPlayer14"));
        assert_eq!(
            values[1],
            expand_registry_path(r"%ProgramFiles%\LDPlayer14")
        );
    }

    #[test]
    fn parses_list2_preserving_vendor_reported_port_separately_from_tcp_serial() {
        let i = parse_instance("0,name,1,2,1,3,5420,900,1600,320").unwrap();
        assert!(i.running);
        assert_eq!(i.width, Some(900));
        assert_eq!(i.height, Some(1600));
        assert_eq!(i.ldplayer_reported_port, Some(5420));
        assert_eq!(
            LdPlayer::tcp_serial_for_index(i.index).as_deref(),
            Some("127.0.0.1:5555")
        );
    }

    #[test]
    fn list2_rejects_invalid_running_state_and_tolerates_optional_bad_columns() {
        assert!(parse_instance("0,测试实例,1,2,running?,3,5555").is_none());
        let parsed = parse_instance("1,中文实例,1,2,true,3,not-a-port,900,1600,320,extra")
            .expect("valid base columns should survive optional malformed port and extra fields");
        assert_eq!(parsed.name, "中文实例");
        assert!(parsed.running);
        assert_eq!(parsed.ldplayer_reported_port, None);
        assert_eq!(parsed.width, Some(900));
        assert_eq!(parsed.height, Some(1600));
        assert_eq!(parsed.dpi, Some(320));
    }
    #[test]
    fn tcp_serial_for_index_rejects_port_overflow() {
        assert_eq!(
            LdPlayer::tcp_serial_for_index(0).as_deref(),
            Some("127.0.0.1:5555")
        );
        assert!(LdPlayer::tcp_serial_for_index(u32::MAX).is_none());
    }
}
