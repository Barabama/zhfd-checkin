use anyhow::{Context, Result, bail};
use std::{
    path::{Path, PathBuf},
    process::Command,
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
    pub adb_port: Option<u16>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub dpi: Option<u32>,
}

impl LdPlayer {
    pub fn discover(configured: &str, root: &Path) -> Result<Self> {
        let mut candidates = Vec::new();
        if !configured.trim().is_empty() {
            let configured_path = PathBuf::from(configured);
            candidates.push(if configured_path.is_dir() {
                configured_path.join("ldconsole.exe")
            } else {
                configured_path
            });
        }
        if let Ok(value) = std::env::var("LDPLAYER_PATH")
            && !value.is_empty()
        {
            candidates.push(PathBuf::from(value));
        }
        candidates.extend([
            root.join("ldconsole.exe"),
            PathBuf::from(r"E:\leidian\LDPlayer14\ldconsole.exe"),
            PathBuf::from(r"C:\LDPlayer\LDPlayer14\ldconsole.exe"),
            PathBuf::from(r"C:\Program Files\LDPlayer\LDPlayer14\ldconsole.exe"),
            PathBuf::from(r"C:\Program Files\LDPlayer\ldconsole.exe"),
        ]);
        for candidate in candidates {
            if candidate.is_file() {
                return Ok(Self {
                    executable: candidate,
                });
            }
        }
        if let Ok(output) = Command::new("where.exe").arg("ldconsole.exe").output()
            && output.status.success()
            && let Some(line) = String::from_utf8_lossy(&output.stdout)
                .lines()
                .find(|l| !l.trim().is_empty())
        {
            let p = PathBuf::from(line.trim());
            if p.is_file() {
                return Ok(Self { executable: p });
            }
        }
        for install_dir in registry_install_locations() {
            let p = install_dir.join("ldconsole.exe");
            if p.is_file() {
                return Ok(Self { executable: p });
            }
        }
        bail!("找不到 LDPlayer ldconsole.exe，请在 config.toml 配置 emulator.ldconsole_path")
    }

    pub fn command(&self, args: &[&str]) -> Result<String> {
        let output = Command::new(&self.executable)
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

    pub fn serial_for_index(index: u32) -> String {
        format!("127.0.0.1:{}", 5555 + index * 2)
    }
}

fn registry_install_locations() -> Vec<PathBuf> {
    let mut result = Vec::new();
    for key in [
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall",
        r"HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall",
        r"HKLM\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    ] {
        let output = Command::new("reg")
            .args(["query", key, "/s", "/v", "InstallLocation"])
            .output();
        let Ok(output) = output else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            if !line.to_ascii_lowercase().contains("installlocation") {
                continue;
            }
            if let Some((_, value)) = line.split_once("REG_SZ") {
                let value = value.trim();
                if !value.is_empty() {
                    result.push(PathBuf::from(value));
                }
            }
        }
    }
    result
}

fn parse_instance(line: &str) -> Option<Instance> {
    let fields: Vec<_> = line.split(',').map(str::trim).collect();
    if fields.len() < 7 {
        return None;
    }
    let index = fields[0].parse().ok()?;
    let running = fields
        .get(4)
        .map(|v| *v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    Some(Instance {
        index,
        name: fields.get(1).unwrap_or(&"").to_string(),
        running,
        adb_port: fields.get(6).and_then(|v| v.parse().ok()),
        width: fields.get(7).and_then(|v| v.parse().ok()),
        height: fields.get(8).and_then(|v| v.parse().ok()),
        dpi: fields.get(9).and_then(|v| v.parse().ok()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_list2() {
        let i = parse_instance("0,name,1,2,1,3,5420,900,1600,320").unwrap();
        assert!(i.running);
        assert_eq!(i.width, Some(900));
        assert_eq!(i.height, Some(1600));
    }
}
