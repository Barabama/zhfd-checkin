use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const CONFIG_BACKUP_LIMIT: usize = 5;
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

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
locating_poll_seconds = 2
location_timeout_seconds = 120
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
    2
}
fn default_location_timeout() -> u64 {
    120
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

impl Config {
    pub fn validate(&self) -> Result<()> {
        if self.app.package_name.trim().is_empty() || self.app.activity_name.trim().is_empty() {
            anyhow::bail!("app.package_name 和 app.activity_name 不能为空");
        }
        crate::domain::Mode::from_config(&self.runtime.mode)?;
        crate::domain::parse_time(&self.window.start)
            .with_context(|| format!("window.start 无效: {}", self.window.start))?;
        crate::domain::parse_time(&self.window.end)
            .with_context(|| format!("window.end 无效: {}", self.window.end))?;
        if self.runtime.locating_poll_seconds == 0
            || self.runtime.location_timeout_seconds == 0
            || self.runtime.startup_timeout_seconds == 0
            || self.runtime.stable_frames == 0
        {
            anyhow::bail!("runtime 的轮询/超时/stable_frames 必须大于 0");
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    pub root: PathBuf,
    pub path: PathBuf,
    pub config: Config,
    pub origin: ConfigOrigin,
    pub created_default: bool,
    pub recovery_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigOrigin {
    Explicit,
    Portable,
    LocalAppData,
}

impl ConfigStore {
    pub fn load() -> Result<Self> {
        Self::load_with_config_path(None)
    }

    pub fn load_with_config_path(config_path: Option<&Path>) -> Result<Self> {
        if let Some(path) = config_path {
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                env::current_dir()?.join(path)
            };
            let root = path
                .parent()
                .context("--config 路径没有父目录")?
                .to_path_buf();
            return Self::load_root(root, path, ConfigOrigin::Explicit, true);
        }

        let portable_root = exe_root()?;
        let portable_config = portable_root.join("config.toml");
        if portable_config.is_file() {
            // A present but invalid portable config must be repaired in place;
            // silently falling back could make the user run against another setup.
            return Self::load_root(
                portable_root,
                portable_config,
                ConfigOrigin::Portable,
                false,
            );
        }

        if let Some(local_root) = local_app_data_root() {
            let local_config = local_root.join("config.toml");
            if local_config.is_file() {
                return Self::load_root(
                    local_root,
                    local_config,
                    ConfigOrigin::LocalAppData,
                    false,
                );
            }
        }

        if probe_writable(&portable_root).is_ok() {
            return Self::load_root(
                portable_root.clone(),
                portable_root.join("config.toml"),
                ConfigOrigin::Portable,
                true,
            );
        }

        let local_root = local_app_data_root().context(
            "EXE 目录不可写，且未找到可用的 LocalAppData；请在 GUI Setup 中选择数据目录",
        )?;
        Self::load_root(
            local_root.clone(),
            local_root.join("config.toml"),
            ConfigOrigin::LocalAppData,
            true,
        )
    }

    fn load_root(
        root: PathBuf,
        path: PathBuf,
        origin: ConfigOrigin,
        create_missing: bool,
    ) -> Result<Self> {
        ensure_runtime_dirs(&root)?;
        if !path.exists() {
            if !create_missing {
                anyhow::bail!("配置文件不存在: {}", path.display());
            }
            let config = Config::default();
            if let Err(error) = write_config(&root, &path, &config) {
                if origin == ConfigOrigin::Portable
                    && local_app_data_root().as_deref() != Some(root.as_path())
                    && let Some(fallback) = local_app_data_root()
                {
                    return Self::load_root(
                        fallback.clone(),
                        fallback.join("config.toml"),
                        ConfigOrigin::LocalAppData,
                        true,
                    );
                }
                return Err(error);
            }
            return Ok(Self {
                root,
                path,
                config,
                origin,
                created_default: true,
                recovery_error: None,
            });
        }
        let text = fs::read_to_string(&path)
            .with_context(|| format!("读取配置失败: {}", path.display()))?;
        let config: Config = toml::from_str(&text)
            .with_context(|| format!("解析 TOML 配置失败: {}", path.display()))?;
        config
            .validate()
            .with_context(|| format!("配置字段不合法: {}", path.display()))?;
        Ok(Self {
            root,
            path,
            config,
            origin,
            created_default: false,
            recovery_error: None,
        })
    }

    pub fn recovery(path: Option<&Path>, error: String) -> Result<Self> {
        let path = match path {
            Some(path) if path.is_absolute() => path.to_path_buf(),
            Some(path) => env::current_dir()?.join(path),
            None => exe_root()?.join("config.toml"),
        };
        let root = path
            .parent()
            .context("修复配置路径没有父目录")?
            .to_path_buf();
        Ok(Self {
            root,
            path,
            config: Config::default(),
            origin: ConfigOrigin::Explicit,
            created_default: false,
            recovery_error: Some(error),
        })
    }

    pub fn load_alternate(path: &Path) -> Result<Self> {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            env::current_dir()?.join(path)
        };
        let root = path.parent().context("配置路径没有父目录")?.to_path_buf();
        Self::load_root(root, path, ConfigOrigin::Explicit, false)
    }

    pub fn save(&self) -> Result<()> {
        write_config(&self.root, &self.path, &self.config)
    }

    /// Persist a candidate first, then publish it as the active in-memory config.
    /// A failed write therefore cannot replace the last known-good configuration.
    pub fn save_candidate(&mut self, candidate: Config) -> Result<()> {
        candidate.validate()?;
        write_config(&self.root, &self.path, &candidate)?;
        self.config = candidate;
        self.recovery_error = None;
        self.created_default = false;
        Ok(())
    }

    pub fn reload(&mut self) -> Result<()> {
        let text = fs::read_to_string(&self.path)
            .with_context(|| format!("读取配置失败: {}", self.path.display()))?;
        let candidate: Config = toml::from_str(&text)
            .with_context(|| format!("解析 TOML 配置失败: {}", self.path.display()))?;
        candidate.validate()?;
        self.config = candidate;
        self.recovery_error = None;
        self.created_default = false;
        Ok(())
    }

    /// Copy this complete data root to a user-selected directory without
    /// changing the current store until the destination has a valid config.
    pub fn migrate_to_root(&self, destination: &Path) -> Result<Self> {
        let destination = if destination.is_absolute() {
            destination.to_path_buf()
        } else {
            env::current_dir()?.join(destination)
        };
        if destination == self.root {
            anyhow::bail!("目标数据目录与当前目录相同: {}", destination.display());
        }
        if is_path_within(&destination, &self.root) || is_path_within(&self.root, &destination) {
            anyhow::bail!(
                "目标数据目录不能是当前目录的父目录或子目录: {}",
                destination.display()
            );
        }
        ensure_runtime_dirs(&destination)?;
        if self.path.is_file() {
            // Keep a recoverable source copy before beginning migration.
            create_config_backup(&self.root, &self.path)?;
        }
        for name in ["logs", "captures", "reports", "backups"] {
            copy_tree_if_missing(&self.root.join(name), &destination.join(name))
                .with_context(|| format!("迁移 {} 目录失败", name))?;
        }
        let destination_config = destination.join("config.toml");
        write_config(&destination, &destination_config, &self.config)?;
        let origin = if local_app_data_root().as_deref() == Some(destination.as_path()) {
            ConfigOrigin::LocalAppData
        } else {
            ConfigOrigin::Explicit
        };
        Self::load_root(destination, destination_config, origin, false)
    }
}

fn is_path_within(candidate: &Path, parent: &Path) -> bool {
    let candidate = candidate
        .canonicalize()
        .unwrap_or_else(|_| candidate.to_path_buf());
    let parent = parent
        .canonicalize()
        .unwrap_or_else(|_| parent.to_path_buf());
    candidate.starts_with(parent)
}

fn copy_tree_if_missing(source: &Path, destination: &Path) -> Result<()> {
    if !source.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_tree_if_missing(&source_path, &destination_path)?;
        } else if !destination_path.exists() {
            fs::copy(&source_path, &destination_path).with_context(|| {
                format!(
                    "复制文件失败: {} -> {}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        }
    }
    Ok(())
}

pub fn recommended_local_app_data_root() -> Option<PathBuf> {
    local_app_data_root()
}

fn local_app_data_root() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
    base.map(|path| path.join("ZHFDCheckin"))
}

fn probe_writable(root: &Path) -> Result<()> {
    fs::create_dir_all(root).with_context(|| format!("无法创建数据目录: {}", root.display()))?;
    let probe = unique_sibling_temp(&root.join(".write-probe"))?;
    let result = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .with_context(|| format!("数据目录不可写: {}", root.display()));
    if result.is_ok() {
        let _ = fs::remove_file(probe);
    }
    result.map(drop)
}

pub fn ensure_runtime_dirs(root: &Path) -> Result<()> {
    for name in ["logs", "captures", "reports", "backups"] {
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

fn write_config(root: &Path, path: &Path, config: &Config) -> Result<()> {
    let parent = path.parent().context("配置文件没有父目录")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("无法创建配置目录: {}", parent.display()))?;
    config.validate()?;
    let text = toml::to_string_pretty(config).context("序列化 TOML 配置失败")?;
    let bytes = text.as_bytes();
    let temp = unique_sibling_temp(path)?;
    let write_result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .with_context(|| format!("无法创建配置临时文件: {}", temp.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("写入配置临时文件失败: {}", temp.display()))?;
        file.sync_all()
            .with_context(|| format!("刷新配置临时文件失败: {}", temp.display()))?;
        drop(file);
        let backup_required = path.is_file();
        if backup_required {
            create_config_backup(root, path)?;
        }
        atomic_replace(&temp, path)
            .with_context(|| format!("原子替换配置失败: {}", path.display()))?;
        if backup_required {
            prune_config_backups(root)?;
        }
        if let Ok(directory) = fs::File::open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    write_result
}

fn unique_sibling_temp(path: &Path) -> Result<PathBuf> {
    let parent = path.parent().context("配置文件没有父目录")?;
    let name = path
        .file_name()
        .context("配置文件没有文件名")?
        .to_string_lossy();
    for _ in 0..100 {
        let serial = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".{}.{}.{}.tmp", name, std::process::id(), serial));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    anyhow::bail!("无法分配唯一配置临时文件名")
}

fn create_config_backup(root: &Path, path: &Path) -> Result<PathBuf> {
    let backup_dir = root.join("backups");
    fs::create_dir_all(&backup_dir)
        .with_context(|| format!("无法创建配置备份目录: {}", backup_dir.display()))?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let mut backup = backup_dir.join(format!("config.toml.bak.{}", stamp));
    let mut suffix = 1u32;
    while backup.exists() {
        backup = backup_dir.join(format!("config.toml.bak.{}-{}", stamp, suffix));
        suffix += 1;
    }
    fs::copy(path, &backup)
        .with_context(|| format!("备份配置失败: {} -> {}", path.display(), backup.display()))?;
    Ok(backup)
}

fn prune_config_backups(root: &Path) -> Result<()> {
    let backup_dir = root.join("backups");
    let mut backups: Vec<_> = fs::read_dir(&backup_dir)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|candidate| {
            candidate
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("config.toml.bak."))
        })
        .collect();
    backups.sort();
    let excess = backups.len().saturating_sub(CONFIG_BACKUP_LIMIT);
    for old in backups.into_iter().take(excess) {
        fs::remove_file(&old).with_context(|| format!("清理旧配置备份失败: {}", old.display()))?;
    }
    Ok(())
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    let source_w: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination_w: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: both paths are NUL-terminated UTF-16 buffers valid for this call.
    let ok = unsafe {
        MoveFileExW(
            source_w.as_ptr(),
            destination_w.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error()).context("MoveFileExW failed");
    }
    Ok(())
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, destination: &Path) -> Result<()> {
    fs::rename(source, destination).context("rename failed")
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
        assert_eq!(config.runtime.locating_poll_seconds, 2);
        assert_eq!(config.runtime.location_timeout_seconds, 120);
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
    fn save_candidate_is_atomic_and_keeps_only_recent_backups() {
        let root = std::env::temp_dir().join(format!("zhfd-config-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("config.toml");
        let mut store = ConfigStore {
            root: root.clone(),
            path: path.clone(),
            config: Config::default(),
            origin: ConfigOrigin::Explicit,
            created_default: false,
            recovery_error: None,
        };
        store.save().unwrap();
        for index in 0..7 {
            let mut candidate = store.config.clone();
            candidate.runtime.stable_frames = (index + 1) as u32;
            store.save_candidate(candidate).unwrap();
        }
        let backups = fs::read_dir(root.join("backups"))
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("config.toml.bak.")
            })
            .count();
        assert_eq!(backups, 5);
        assert_eq!(
            toml::from_str::<Config>(&fs::read_to_string(path).unwrap())
                .unwrap()
                .runtime
                .stable_frames,
            7
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn migrate_to_root_copies_data_and_keeps_source() {
        let source =
            std::env::temp_dir().join(format!("zhfd-migrate-source-{}", std::process::id()));
        let destination =
            std::env::temp_dir().join(format!("zhfd-migrate-destination-{}", std::process::id()));
        let _ = fs::remove_dir_all(&source);
        let _ = fs::remove_dir_all(&destination);
        fs::create_dir_all(source.join("logs")).unwrap();
        fs::write(
            source.join("config.toml"),
            toml::to_string_pretty(&Config::default()).unwrap(),
        )
        .unwrap();
        fs::write(source.join("logs/example.txt"), "evidence").unwrap();
        let store = ConfigStore {
            root: source.clone(),
            path: source.join("config.toml"),
            config: Config::default(),
            origin: ConfigOrigin::Explicit,
            created_default: false,
            recovery_error: None,
        };
        let migrated = store.migrate_to_root(&destination).unwrap();
        assert_eq!(migrated.root, destination);
        assert!(migrated.path.is_file());
        assert_eq!(
            fs::read_to_string(destination.join("logs/example.txt")).unwrap(),
            "evidence"
        );
        assert!(source.join("config.toml").is_file());
        let _ = fs::remove_dir_all(source);
        let _ = fs::remove_dir_all(destination);
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
