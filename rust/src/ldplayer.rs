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
        Ok(decode_console_output(&output.stdout))
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

/// Decode `ldconsole` console output.
///
/// `ldconsole.exe` writes through the Windows console code page, which is CP936
/// on a Simplified-Chinese host, so instance names arrive as CP936 bytes instead
/// of UTF-8 and `from_utf8_lossy` would turn them into U+FFFD mojibake. Bytes
/// that are already valid UTF-8 always win, which keeps UTF-8 hosts, ASCII
/// output, and every existing fixture untouched.
///
/// This is scoped to the ldconsole console stream only. ADB output, stderr and
/// all JSON/TOML files stay strict UTF-8.
fn decode_console_output(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }
    #[cfg(windows)]
    {
        // `ldconsole.exe` writes bytes in the host ANSI code page, which is
        // CP936 on a Simplified-Chinese Windows. Order matters: the host code
        // page is authoritative when it is a real ANSI page, and CP936 is the
        // fallback for a UTF-8 host (ACP 65001), where the vendor tool may
        // still emit its own code page. Attempting CP936 first on a non-Chinese
        // host would turn correct host-encoded names into mojibake instead of
        // letting the host page decode them.
        let host_code_page = unsafe { GetACP() };
        let mut code_pages = Vec::with_capacity(2);
        if host_code_page != 65001 {
            code_pages.push(host_code_page);
        }
        if host_code_page != 936 {
            code_pages.push(936);
        }
        for code_page in code_pages {
            if let Some(text) = decode_with_code_page(code_page, bytes) {
                return text;
            }
        }
    }
    String::from_utf8_lossy(bytes).to_string()
}

/// Decode with an explicit code page, rejecting the attempt when the bytes are
/// not valid in it.
///
/// `MB_ERR_INVALID_CHARS` is essential: without it `MultiByteToWideChar`
/// substitutes invalid sequences and returns success for almost any input, so
/// the first attempted code page would always win and the fallback order would
/// be meaningless.
#[cfg(windows)]
fn decode_with_code_page(code_page: u32, bytes: &[u8]) -> Option<String> {
    use std::os::windows::ffi::OsStringExt;
    const MB_ERR_INVALID_CHARS: u32 = 0x0000_0008;
    // SAFETY: `bytes` is a valid slice for the given length; the first call only
    // measures, and the wide buffer is allocated from the exact returned count.
    let len = unsafe {
        MultiByteToWideChar(
            code_page,
            MB_ERR_INVALID_CHARS,
            bytes.as_ptr(),
            bytes.len() as i32,
            std::ptr::null_mut(),
            0,
        )
    };
    if len <= 0 {
        return None;
    }
    let mut wide = vec![0u16; len as usize];
    let written = unsafe {
        MultiByteToWideChar(
            code_page,
            MB_ERR_INVALID_CHARS,
            bytes.as_ptr(),
            bytes.len() as i32,
            wide.as_mut_ptr(),
            len,
        )
    };
    if written <= 0 {
        return None;
    }
    wide.truncate(written as usize);
    Some(
        std::ffi::OsString::from_wide(&wide)
            .to_string_lossy()
            .into_owned(),
    )
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn MultiByteToWideChar(
        code_page: u32,
        flags: u32,
        multi_byte_str: *const u8,
        multi_byte_count: i32,
        wide_char_str: *mut u16,
        wide_char_count: i32,
    ) -> i32;
    fn GetACP() -> u32;
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

    #[test]
    fn console_output_keeps_valid_utf8_and_ascii_untouched() {
        assert_eq!(
            decode_console_output(b"0,instance,1,2,1,3,5420"),
            "0,instance,1,2,1,3,5420"
        );
        let utf8 = "0,中文实例,1,2,1,3,5420".as_bytes();
        assert_eq!(decode_console_output(utf8), "0,中文实例,1,2,1,3,5420");
    }

    #[cfg(windows)]
    #[test]
    fn decode_with_code_page_rejects_invalid_bytes_and_accepts_valid_ones() {
        // Without MB_ERR_INVALID_CHARS the API substitutes instead of failing,
        // which would make the first attempted code page always win and the
        // fallback order meaningless. 0xFF is not a valid CP936 lead byte.
        assert!(decode_with_code_page(936, &[0xFF, 0xFE, 0xFD]).is_none());
        assert_eq!(
            decode_with_code_page(936, b"0,instance,1,2,1,3,5420").as_deref(),
            Some("0,instance,1,2,1,3,5420")
        );
    }

    /// CP936 bytes for `0,中文签到实例,1,2,1,3,5420`, i.e. what `ldconsole list2`
    /// emits when the host console code page is 936.
    #[cfg(windows)]
    #[test]
    fn console_output_decodes_cp936_instance_names_without_mojibake() {
        const CP936: &[u8] = b"0,\xD6\xD0\xCE\xC4\xC7\xA9\xB5\xBD\xCA\xB5\xC0\xFD,1,2,1,3,5420";
        // The fixture is deliberately not valid UTF-8: decoding it as UTF-8
        // yields U+FFFD, which is exactly the mojibake this decoder avoids.
        assert!(String::from_utf8_lossy(CP936).contains('\u{FFFD}'));
        // CP936 decoding itself is host-independent and must always recover the
        // real instance name.
        assert_eq!(
            decode_with_code_page(936, CP936).as_deref(),
            Some("0,中文签到实例,1,2,1,3,5420")
        );
        // `decode_console_output` prefers the host ANSI page, so the CP936
        // fallback is only reached on a host whose page cannot represent these
        // bytes (a UTF-8 host) or is CP936 itself. Asserting the Chinese name
        // unconditionally would fail on every other host, including the en-US
        // CI runners: their ANSI page (1252) defines all of these bytes, so it
        // succeeds with mojibake instead of falling through to CP936.
        let host_code_page = unsafe { GetACP() };
        if host_code_page == 936 || host_code_page == 65001 {
            assert_eq!(decode_console_output(CP936), "0,中文签到实例,1,2,1,3,5420");
            assert!(!decode_console_output(CP936).contains('\u{FFFD}'));
        } else {
            // Documented tradeoff: the host page wins and the name stays
            // mojibake rather than being guessed as CP936.
            assert!(!decode_console_output(CP936).is_empty());
        }
    }
}
