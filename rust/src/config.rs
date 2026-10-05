use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

pub const DEFAULT_CONFIG: &str = r#"[app]
package_name = "cn.edu.fzu.fdxypa"
activity_name = "cn.edu.fzu.fdxy_app.MainActivity"
apk_path = ""

[emulator]
ldconsole_path = ""
adb_path = ""
instance_index = 0
serial = ""
auto_launch = true

[window]
start = "21:00"
end = "23:59"
timezone = "Asia/Shanghai"

[runtime]
mode = "dry-run"
locating_poll_seconds = 10
location_timeout_seconds = 150
success_timeout_seconds = 60
startup_timeout_seconds = 30
stable_frames = 2
unknown_retries = 3

[notifications]
bark_url = ""
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub app: AppConfig,
    pub emulator: EmulatorConfig,
    pub window: WindowConfig,
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub notifications: NotificationConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub package_name: String,
    pub activity_name: String,
    #[serde(default)]
    pub apk_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmulatorConfig {
    #[serde(default)]
    pub ldconsole_path: String,
    #[serde(default)]
    pub adb_path: String,
    #[serde(default)]
    pub instance_index: u32,
    #[serde(default)]
    pub serial: String,
    #[serde(default = "default_true")]
    pub auto_launch: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instances: Vec<EmulatorInstanceConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmulatorInstanceConfig {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub instance_index: u32,
    #[serde(default)]
    pub serial: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl EmulatorConfig {
    pub fn effective_instances(&self) -> Vec<EmulatorInstanceConfig> {
        if self.instances.is_empty() {
            return vec![EmulatorInstanceConfig {
                name: "default".into(),
                instance_index: self.instance_index,
                serial: self.serial.clone(),
                enabled: true,
            }];
        }
        self.instances
            .iter()
            .filter(|instance| instance.enabled)
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowConfig {
    pub start: String,
    pub end: String,
    #[serde(default = "default_timezone")]
    pub timezone: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default = "default_locating_poll")]
    pub locating_poll_seconds: u64,
    #[serde(default = "default_location_timeout")]
    pub location_timeout_seconds: u64,
    #[serde(default = "default_success_timeout")]
    pub success_timeout_seconds: u64,
    #[serde(default = "default_startup_timeout")]
    pub startup_timeout_seconds: u64,
    #[serde(default = "default_stable_frames")]
    pub stable_frames: u32,
    #[serde(default = "default_unknown_retries")]
    pub unknown_retries: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NotificationConfig {
    #[serde(default)]
    pub bark_url: String,
}

fn default_true() -> bool {
    true
}
fn default_timezone() -> String {
    "Asia/Shanghai".to_string()
}
fn default_mode() -> String {
    "dry-run".to_string()
}
fn default_locating_poll() -> u64 {
    10
}
fn default_location_timeout() -> u64 {
    150
}
fn default_success_timeout() -> u64 {
    60
}
fn default_startup_timeout() -> u64 {
    30
}
fn default_stable_frames() -> u32 {
    2
}
fn default_unknown_retries() -> u32 {
    3
}

impl Default for Config {
    fn default() -> Self {
        toml::from_str(DEFAULT_CONFIG).expect("embedded default config must be valid")
    }
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    pub root: PathBuf,
    pub path: PathBuf,
    pub config: Config,
}

impl ConfigStore {
    pub fn load() -> Result<Self> {
        let root = exe_root()?;
        ensure_runtime_dirs(&root)?;
        let path = root.join("config.toml");
        if !path.exists() {
            let config = Config::default();
            write_config(&path, &config)?;
            return Ok(Self { root, path, config });
        }
        let text = fs::read_to_string(&path)
            .with_context(|| format!("读取配置失败: {}", path.display()))?;
        let config: Config = toml::from_str(&text)
            .with_context(|| format!("解析 TOML 配置失败: {}", path.display()))?;
        Ok(Self { root, path, config })
    }

    pub fn save(&self) -> Result<()> {
        write_config(&self.path, &self.config)
    }
}

pub fn ensure_runtime_dirs(root: &Path) -> Result<()> {
    for name in ["logs", "captures", "reports"] {
        fs::create_dir_all(root.join(name))
            .with_context(|| format!("无法创建运行目录: {}", root.join(name).display()))?;
    }
    Ok(())
}

pub fn exe_root() -> Result<PathBuf> {
    let exe = env::current_exe().context("无法取得 EXE 路径")?;
    exe.parent()
        .map(Path::to_path_buf)
        .context("EXE 没有父目录")
}

fn write_config(path: &Path, config: &Config) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("无法创建配置目录: {}", parent.display()))?;
    }
    let text = toml::to_string_pretty(config).context("序列化 TOML 配置失败")?;
    fs::write(path, text).with_context(|| format!("无法写入配置: {}", path.display()))
}

pub fn resolve_path(root: &Path, value: &str) -> Option<PathBuf> {
    if value.trim().is_empty() {
        return None;
    }
    let p = PathBuf::from(value);
    if p.is_absolute() {
        Some(p)
    } else {
        Some(root.join(p))
    }
}

pub fn path_for_config(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_defaults_are_safe_and_match_migration_policy() {
        let config = Config::default();
        assert_eq!(config.runtime.mode, "dry-run");
        assert_eq!(config.runtime.locating_poll_seconds, 10);
        assert_eq!(config.runtime.location_timeout_seconds, 150);
        assert_eq!(config.runtime.success_timeout_seconds, 60);
        assert_eq!(config.runtime.stable_frames, 2);
        assert_eq!(config.window.start, "21:00");
        assert_eq!(config.window.end, "23:59");
        assert_eq!(config.window.timezone, "Asia/Shanghai");
        assert_eq!(config.app.package_name, "cn.edu.fzu.fdxypa");
    }

    #[test]
    fn legacy_single_instance_config_becomes_default_effective_instance() {
        let config = Config::default();
        let instances = config.emulator.effective_instances();
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].instance_index, 0);
        assert!(instances[0].enabled);
    }

    #[test]
    fn multi_instance_config_round_trips_as_array_of_tables() {
        let mut config = Config::default();
        config.emulator.instances = vec![
            EmulatorInstanceConfig {
                name: "one".into(),
                instance_index: 0,
                serial: "".into(),
                enabled: true,
            },
            EmulatorInstanceConfig {
                name: "two".into(),
                instance_index: 1,
                serial: "127.0.0.1:5557".into(),
                enabled: true,
            },
        ];
        let text = toml::to_string_pretty(&config).unwrap();
        assert!(text.contains("[[emulator.instances]]"));
        let parsed: Config = toml::from_str(&text).unwrap();
        assert_eq!(parsed.emulator.effective_instances().len(), 2);
        assert_eq!(parsed.emulator.instances[1].serial, "127.0.0.1:5557");
    }

    #[test]
    fn configured_instances_filter_disabled_entries() {
        let mut config = Config::default();
        config.emulator.instances = vec![
            EmulatorInstanceConfig {
                name: "one".into(),
                instance_index: 0,
                serial: "".into(),
                enabled: true,
            },
            EmulatorInstanceConfig {
                name: "two".into(),
                instance_index: 1,
                serial: "".into(),
                enabled: false,
            },
        ];
        let instances = config.emulator.effective_instances();
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].name, "one");
    }

    #[test]
    fn apk_config_path_is_relative_only_when_inside_exe_root() {
        let root = Path::new(r"C:\portable\zhfd");
        assert_eq!(
            path_for_config(root, &root.join(r"apps\zhfd.apk")),
            "apps/zhfd.apk"
        );
        assert_eq!(
            path_for_config(root, Path::new(r"D:\downloads\zhfd.apk")),
            r"D:\downloads\zhfd.apk"
        );
    }

    #[test]
    fn empty_apk_config_path_resolves_to_none() {
        assert!(resolve_path(Path::new("."), "  ").is_none());
        assert_eq!(
            resolve_path(Path::new("root"), "app.apk"),
            Some(PathBuf::from("root/app.apk"))
        );
    }
}
