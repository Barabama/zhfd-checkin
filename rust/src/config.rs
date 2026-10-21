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
# Field work ("外勤签到") uses a different live control. It stays disabled by
# default; set true only after confirming the field-work action, device, location,
# instance, and consequences for the account. See CHECKIN_SAFETY.md.
allow_field_work_live = false
# One-click login on the login page is opt-in. It never stores or types a
# password. See ADR-0004 and docs/CHECKIN_SAFETY.md.
allow_auto_login = false
# Separate consent switch: only true lets the program tick the User/Privacy
# agreement on the login page on the operator's behalf.
accept_login_terms = false
# Optional expected "last used account"; empty means no match check.
# Local-only value: never commit this file.
auto_login_account = ""
# Seconds to wait for the login page to disappear, clamped to 5-120.
auto_login_timeout_seconds = 30

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
    /// Opt-in switch for tapping the field-work ("外勤签到") control in live
    /// runs. Defaults to `false`: field work is a different business action
    /// from the ordinary evening check-in, so enabling it is an explicit
    /// operator decision recorded in the config file.
    #[serde(default)]
    pub allow_field_work_live: bool,
    /// Opt-in switch for one-click login on the login page. Defaults to
    /// `false`; older configs without the key stay denied.
    #[serde(default)]
    pub allow_auto_login: bool,
    /// Separate consent switch: when `true`, the program may tick the
    /// User/Privacy agreement checkbox on the login page before tapping
    /// one-click login. Kept separate so `allow_auto_login` never implies
    /// agreement on the operator's behalf.
    #[serde(default)]
    pub accept_login_terms: bool,
    /// Optional expected "last used account" label. Empty means no match
    /// check. The value is local-only and must never be committed.
    #[serde(default)]
    pub auto_login_account: String,
    /// How long to wait for the login page to disappear after the tap.
    /// Clamped to 5-120 seconds where it is used.
    #[serde(default = "default_auto_login_timeout")]
    pub auto_login_timeout_seconds: u64,
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
fn default_auto_login_timeout() -> u64 {
    30
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

    /// A copy safe for screens, logs, and shareable diagnostic output.
    ///
    /// The on-disk config keeps the real values; only display/export copies are
    /// redacted. `auto_login_account` is PII (student account) and `bark_url`
    /// can embed a webhook token.
    pub fn redacted_for_display(&self) -> Self {
        let mut redacted = self.clone();
        if !redacted.runtime.auto_login_account.is_empty() {
            redacted.runtime.auto_login_account = "<redacted>".into();
        }
        if !redacted.notifications.bark_url.is_empty() {
            redacted.notifications.bark_url = "<redacted>".into();
        }
        redacted
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
    /// A data root chosen through the GUI migration flow. It is recorded in a
    /// per-user pointer file so a restart does not silently fall back to the
    /// source root (ADR-0003).
    Recorded,
}

impl ConfigOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::Portable => "portable",
            Self::LocalAppData => "local_app_data",
            Self::Recorded => "recorded",
        }
    }
}

impl ConfigStore {
    /// Resolve the config path, origin and existence without parsing or
    /// creating the file. Used by `config path` so troubleshooting works even
    /// when the config is damaged.
    pub fn resolve_path(explicit: Option<&Path>) -> Result<(PathBuf, ConfigOrigin, bool)> {
        if let Some(path) = explicit {
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                env::current_dir()?.join(path)
            };
            let exists = path.is_file();
            return Ok((path, ConfigOrigin::Explicit, exists));
        }
        let recorded = read_recorded_root();
        let portable = exe_root()?.join("config.toml");
        let local = local_app_data_root().map(|root| root.join("config.toml"));
        let (path, origin) = if let Some(recorded) = recorded {
            (recorded, ConfigOrigin::Recorded)
        } else if portable.is_file() {
            (portable, ConfigOrigin::Portable)
        } else if let Some(local) = local.filter(|path| path.is_file()) {
            (local, ConfigOrigin::LocalAppData)
        } else {
            (portable, ConfigOrigin::Portable)
        };
        let exists = path.is_file();
        Ok((path, origin, exists))
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
        // A root chosen through the GUI migration flow wins over the portable
        // and LocalAppData candidates; otherwise a successful migration would
        // silently revert on the next start. Explicit `--config` still wins.
        if let Some(recorded_config) = read_recorded_root() {
            let root = recorded_config
                .parent()
                .context("已记录的数据目录没有父目录")?
                .to_path_buf();
            return Self::load_root(root, recorded_config, ConfigOrigin::Recorded, false);
        }

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

    /// Build the Repair handle for a config that failed to load.
    ///
    /// The path must be the SAME file `load_with_config_path` actually tried,
    /// otherwise the Repair page would display, reload and overwrite a
    /// different file than the damaged one (frozen decision D-01/D-03). So when
    /// no explicit path is given the same candidate order is re-walked here.
    pub fn recovery(path: Option<&Path>, error: String) -> Result<Self> {
        let path = match path {
            Some(path) if path.is_absolute() => path.to_path_buf(),
            Some(path) => env::current_dir()?.join(path),
            None => Self::default_config_path()?,
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

    /// The config path `load_with_config_path(None)` would use, without
    /// requiring it to be valid.
    ///
    /// Mirrors that function's priority chain: an existing portable config, an
    /// existing LocalAppData config, then the portable candidate (which is also
    /// the fallback when the EXE directory turns out to be unwritable).
    pub fn default_config_path() -> Result<PathBuf> {
        let portable_root = exe_root()?;
        let portable_config = portable_root.join("config.toml");
        if portable_config.is_file() {
            return Ok(portable_config);
        }
        if let Some(local_root) = local_app_data_root() {
            let local_config = local_root.join("config.toml");
            if local_config.is_file() {
                return Ok(local_config);
            }
        }
        Ok(portable_config)
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

    /// Overwrite the on-disk config with the current in-memory value.
    ///
    /// A store built by [`ConfigStore::recovery`] holds `Config::default()`,
    /// not the user's damaged file. Writing it would silently destroy the
    /// original settings, so ordinary saves are refused while a recovery error
    /// is present. Repair must go through [`ConfigStore::save_candidate`], which
    /// is the explicit "apply this configuration" action.
    pub fn save(&self) -> Result<()> {
        if let Some(error) = &self.recovery_error {
            anyhow::bail!(
                "配置文件尚未修复，拒绝用默认配置覆盖: {}（原始错误: {}）",
                self.path.display(),
                error
            );
        }
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
        // A recovery store holds defaults, not the user's damaged file. Copying
        // it would create a destination config that never existed.
        if let Some(error) = &self.recovery_error {
            anyhow::bail!(
                "配置文件尚未修复，拒绝迁移: {}（原始错误: {}）",
                self.path.display(),
                error
            );
        }
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
        // Reject an occupied destination BEFORE writing anything into it: a
        // refused migration must leave the destination exactly as the user left
        // it, not half-populated with copied evidence directories.
        let destination_config = destination.join("config.toml");
        if destination_config.is_file() {
            anyhow::bail!(
                "目标数据目录已有 config.toml，迁移已停止且未覆盖目标配置: {}",
                destination_config.display()
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
        write_config(&destination, &destination_config, &self.config)?;
        let origin = if local_app_data_root().as_deref() == Some(destination.as_path()) {
            ConfigOrigin::LocalAppData
        } else {
            ConfigOrigin::Recorded
        };
        Self::load_root(destination, destination_config, origin, false)
    }

    /// Persist this store's config path as the user's chosen data root
    /// (ADR-0003).
    ///
    /// Kept separate from [`ConfigStore::migrate_to_root`] so the library test
    /// suite never touches the real `%LOCALAPPDATA%` pointer. The GUI calls this
    /// after a successful migration; a failure means the copy succeeded but the
    /// next start would fall back to the source root.
    pub fn record_current_root(&self) -> Result<()> {
        record_root(&self.path)
    }
}

fn is_path_within(candidate: &Path, parent: &Path) -> bool {
    // Canonicalize both sides together: a non-existent destination cannot be
    // canonicalized, and mixing a canonicalized `\\?\` path with a raw path
    // would never match, silently disabling the nesting guard.
    match (candidate.canonicalize(), parent.canonicalize()) {
        (Ok(candidate), Ok(parent)) => path_prefix_matches(&candidate, &parent),
        _ => path_prefix_matches(
            &lexically_normalize(candidate),
            &lexically_normalize(parent),
        ),
    }
}

fn lexically_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn path_prefix_matches(candidate: &Path, parent: &Path) -> bool {
    let candidate = candidate.to_string_lossy().replace('/', "\\");
    let parent = parent.to_string_lossy().replace('/', "\\");
    let candidate = candidate.trim_end_matches('\\');
    let parent = parent.trim_end_matches('\\');
    let (candidate, parent) = if cfg!(windows) {
        (candidate.to_lowercase(), parent.to_lowercase())
    } else {
        (candidate.to_string(), parent.to_string())
    };
    candidate == parent
        || candidate
            .strip_prefix(&parent)
            .is_some_and(|rest| rest.starts_with('\\'))
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

/// Per-user file that records the data root chosen through migration.
///
/// It lives in the LocalAppData root rather than beside the EXE so moving the
/// portable directory does not silently change which data root is used.
fn recorded_root_file() -> Option<PathBuf> {
    local_app_data_root().map(|root| root.join("data-root.txt"))
}

/// Absolute config path previously chosen by the user, if it still exists.
///
/// A missing, unreadable, relative, or nonexistent pointer is ignored so a
/// broken pointer can never block startup; the normal candidate chain applies.
fn read_recorded_root() -> Option<PathBuf> {
    read_recorded_root_from(&recorded_root_file()?)
}

fn read_recorded_root_from(file: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string(file).ok()?;
    let path = PathBuf::from(text.trim());
    (path.is_absolute() && path.is_file()).then_some(path)
}

/// Persist the migrated config path (ADR-0003).
fn record_root(config_path: &Path) -> Result<()> {
    record_root_in(
        &recorded_root_file().context("无法确定数据目录指针位置")?,
        config_path,
    )
}

fn record_root_in(file: &Path, config_path: &Path) -> Result<()> {
    let parent = file.parent().context("数据目录指针没有父目录")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("无法创建数据目录指针目录: {}", parent.display()))?;
    let temp = unique_sibling_temp(file)?;
    let text = format!("{}\n", config_path.display());
    let written = (|| -> Result<()> {
        fs::write(&temp, text.as_bytes())
            .with_context(|| format!("写入数据目录指针失败: {}", temp.display()))?;
        if let Ok(handle) = fs::File::open(&temp) {
            let _ = handle.sync_all();
        }
        atomic_replace(&temp, file)
            .with_context(|| format!("原子替换数据目录指针失败: {}", file.display()))
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
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
    // Backups are long-lived copies outside the config screen. Keep the
    // structural content for recovery, but never persist the account or the
    // notification webhook token. A damaged/non-UTF-8 file falls back to a raw
    // copy so Repair can still recover it.
    match fs::read_to_string(path) {
        Ok(text) => {
            fs::write(&backup, redact_sensitive_config_text(&text)).with_context(|| {
                format!("备份配置失败: {} -> {}", path.display(), backup.display())
            })?;
        }
        Err(_) => {
            fs::copy(path, &backup).with_context(|| {
                format!("备份配置失败: {} -> {}", path.display(), backup.display())
            })?;
        }
    }
    Ok(backup)
}

/// Replace the values of PII/secret keys in a TOML document while keeping the
/// rest of the file byte-for-byte usable for recovery.
fn redact_sensitive_config_text(text: &str) -> String {
    const SENSITIVE_KEYS: [&str; 2] = ["auto_login_account", "bark_url"];
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let trimmed = line.trim_start();
        let indent = &line[..line.len() - trimmed.len()];
        let mut redacted = false;
        for key in SENSITIVE_KEYS {
            if let Some(rest) = trimmed.strip_prefix(key)
                && rest.trim_start().starts_with('=')
            {
                out.push_str(indent);
                out.push_str(key);
                out.push_str(" = \"<redacted>\"");
                redacted = true;
                break;
            }
        }
        if !redacted {
            out.push_str(line);
        }
        out.push('\n');
    }
    if !text.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    out
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
        // Field-work tapping is opt-in: the embedded default must never enable
        // it silently, and an older config written before the key existed must
        // still deserialize to "denied".
        assert!(!config.runtime.allow_field_work_live);
        let legacy: RuntimeConfig = toml::from_str("mode = \"live\"\n").unwrap();
        assert!(!legacy.allow_field_work_live);
        // Auto-login and consent are separate opt-ins and must both default to
        // denied; a legacy config lacking the keys must not enable either.
        assert!(!config.runtime.allow_auto_login);
        assert!(!config.runtime.accept_login_terms);
        assert!(config.runtime.auto_login_account.is_empty());
        assert_eq!(config.runtime.auto_login_timeout_seconds, 30);
        assert!(!legacy.allow_auto_login);
        assert!(!legacy.accept_login_terms);
        assert!(legacy.auto_login_account.is_empty());
        assert_eq!(legacy.auto_login_timeout_seconds, 30);
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
    fn save_is_refused_while_a_recovery_error_is_present() {
        let root = std::env::temp_dir().join(format!("zhfd-recovery-guard-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("config.toml");
        fs::write(&path, "[broken").unwrap();
        let mut store = ConfigStore {
            root: root.clone(),
            path: path.clone(),
            config: Config::default(),
            origin: ConfigOrigin::Explicit,
            created_default: false,
            recovery_error: Some("parse error".into()),
        };
        // A recovery store must never overwrite the damaged file with defaults.
        assert!(store.save().is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "[broken");
        // The explicit repair action is still allowed and clears the error.
        store.save_candidate(Config::default()).unwrap();
        assert!(store.recovery_error.is_none());
        assert!(fs::read_to_string(&path).unwrap().contains("package_name"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recorded_root_round_trips_and_invalid_pointers_are_ignored() {
        let root = std::env::temp_dir().join(format!("zhfd-recorded-root-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let pointer = root.join("data-root.txt");
        let config = root.join("root").join("config.toml");
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        fs::write(&config, "config").unwrap();

        record_root_in(&pointer, &config).unwrap();
        assert_eq!(read_recorded_root_from(&pointer), Some(config.clone()));

        // A relative pointer is refused.
        fs::write(&pointer, "config.toml\n").unwrap();
        assert_eq!(read_recorded_root_from(&pointer), None);

        // A pointer to a missing file is refused.
        fs::write(&pointer, root.join("missing.toml").display().to_string()).unwrap();
        assert_eq!(read_recorded_root_from(&pointer), None);

        // A missing pointer file is refused, not an error.
        fs::remove_file(&pointer).unwrap();
        assert_eq!(read_recorded_root_from(&pointer), None);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn display_copy_redacts_account_and_notification_url() {
        let mut config = Config::default();
        config.runtime.auto_login_account = "123456789".into();
        config.notifications.bark_url = "https://example.invalid/secret".into();
        let redacted = config.redacted_for_display();
        assert_eq!(redacted.runtime.auto_login_account, "<redacted>");
        assert_eq!(redacted.notifications.bark_url, "<redacted>");
        // The real config must keep the values; only the display copy changes.
        assert_eq!(config.runtime.auto_login_account, "123456789");
        assert_eq!(
            config.notifications.bark_url,
            "https://example.invalid/secret"
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
        store.config.runtime.auto_login_account = "123456789".into();
        store.config.notifications.bark_url = "https://example.invalid/secret".into();
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
        let mut saw_redaction = false;
        for entry in fs::read_dir(root.join("backups")).unwrap().flatten() {
            let text = fs::read_to_string(entry.path()).unwrap();
            assert!(
                !text.contains("123456789"),
                "backup leaked the account: {}",
                entry.path().display()
            );
            assert!(
                !text.contains("https://example.invalid/secret"),
                "backup leaked the webhook URL: {}",
                entry.path().display()
            );
            saw_redaction |= text.contains("<redacted>");
        }
        assert!(saw_redaction, "expected at least one redacted backup");
        assert_eq!(
            toml::from_str::<Config>(&fs::read_to_string(&path).unwrap())
                .unwrap()
                .runtime
                .stable_frames,
            7
        );
        // The active config keeps the real values; only backups are redacted.
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains("auto_login_account = \"123456789\"")
        );
        let _ = fs::remove_dir_all(root);
    }

    // ===== v0.3.0 independent audit counterexamples =====

    fn audit_store(root: &Path, config: Config) -> ConfigStore {
        ConfigStore {
            root: root.to_path_buf(),
            path: root.join("config.toml"),
            config,
            origin: ConfigOrigin::Explicit,
            created_default: false,
            recovery_error: None,
        }
    }

    #[test]
    fn audit_migration_must_not_silently_replace_existing_destination_config() {
        let source = std::env::temp_dir().join(format!(
            "zhfd-audit-mig-src-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let destination = std::env::temp_dir().join(format!(
            "zhfd-audit-mig-dst-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&source);
        let _ = fs::remove_dir_all(&destination);
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&destination).unwrap();

        let mut source_config = Config::default();
        source_config.runtime.stable_frames = 7;
        fs::write(
            source.join("config.toml"),
            toml::to_string_pretty(&source_config).unwrap(),
        )
        .unwrap();

        let mut destination_config = Config::default();
        destination_config.runtime.stable_frames = 1;
        fs::write(
            destination.join("config.toml"),
            toml::to_string_pretty(&destination_config).unwrap(),
        )
        .unwrap();

        let store = audit_store(&source, source_config);
        let result = store.migrate_to_root(&destination);

        let destination_after = fs::read_to_string(destination.join("config.toml")).unwrap();
        let parsed: Config = toml::from_str(&destination_after).unwrap();
        let _ = fs::remove_dir_all(&source);
        let _ = fs::remove_dir_all(&destination);

        // Safe expectation: an existing destination config is never silently
        // replaced; migration must fail or preserve it pending user confirmation.
        assert!(
            result.is_err() || parsed.runtime.stable_frames == 1,
            "迁移静默用源配置覆盖了目标目录已有 config.toml（stable_frames={}）",
            parsed.runtime.stable_frames
        );
    }

    #[test]
    fn audit_explicit_invalid_config_is_not_silently_defaulted() {
        let root = std::env::temp_dir().join(format!(
            "zhfd-audit-invalid-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("config.toml");
        fs::write(&path, "[not valid toml").unwrap();
        let result = ConfigStore::load_with_config_path(Some(&path));
        let bytes_after = fs::read_to_string(&path).unwrap();
        let _ = fs::remove_dir_all(&root);
        assert!(result.is_err(), "损坏的显式配置被静默接受");
        assert_eq!(bytes_after, "[not valid toml", "损坏的显式配置被改写");
    }

    #[test]
    fn audit_migration_rejects_child_and_parent_targets() {
        let root = std::env::temp_dir().join(format!(
            "zhfd-audit-nest-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let store = audit_store(&root, Config::default());
        assert!(store.migrate_to_root(&root.join("child")).is_err());
        assert!(store.migrate_to_root(root.parent().unwrap()).is_err());
        let _ = fs::remove_dir_all(&root);
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

    // ===== v0.3.0 first-run / recovery counterexamples (D-01/D-03/D-06) =====

    #[test]
    fn explicit_config_path_uses_its_parent_as_data_root() {
        let root = std::env::temp_dir().join(format!(
            "zhfd-explicit-root-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        let data_root = root.join("UserData");
        fs::create_dir_all(&data_root).unwrap();
        let config_path = data_root.join("config.toml");
        fs::write(&config_path, DEFAULT_CONFIG).unwrap();

        let store = ConfigStore::load_with_config_path(Some(&config_path)).unwrap();
        assert_eq!(
            store.root, data_root,
            "显式 --config 的父目录必须是 data root"
        );
        assert_eq!(store.path, config_path);
        assert_eq!(store.origin, ConfigOrigin::Explicit);
        for name in ["logs", "captures", "reports", "backups"] {
            assert!(
                data_root.join(name).is_dir(),
                "显式 data root 必须创建 {name} 目录"
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn damaged_config_enters_recovery_without_touching_the_file() {
        let root = std::env::temp_dir().join(format!(
            "zhfd-recovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("config.toml");
        let damaged = "[app]\npackage_name = 1\n";
        fs::write(&path, damaged).unwrap();

        let load_error = ConfigStore::load_with_config_path(Some(&path)).unwrap_err();
        let store = ConfigStore::recovery(Some(&path), format!("{load_error:#}")).unwrap();

        assert_eq!(store.path, path);
        assert_eq!(store.root, root);
        assert!(store.recovery_error.is_some(), "修复模式必须携带原始错误");
        assert!(!store.created_default);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            damaged,
            "进入修复模式不得改写损坏文件"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn recovery_save_candidate_repairs_in_place_and_keeps_a_backup() {
        let root = std::env::temp_dir().join(format!(
            "zhfd-recovery-save-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("config.toml");
        let damaged = "not = valid = toml";
        fs::write(&path, damaged).unwrap();

        let mut store = ConfigStore::recovery(Some(&path), "解析失败".into()).unwrap();
        let mut candidate = Config::default();
        candidate.emulator.ldconsole_path = "tools/ldconsole.exe".into();
        store.save_candidate(candidate).unwrap();

        assert!(store.recovery_error.is_none(), "修复成功后必须清除错误状态");
        let reloaded: Config = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(reloaded.emulator.ldconsole_path, "tools/ldconsole.exe");
        let backups: Vec<_> = fs::read_dir(root.join("backups"))
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("config.toml.bak.")
            })
            .collect();
        assert_eq!(backups.len(), 1, "覆盖已存在配置前必须保留一份备份");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn failed_migration_leaves_the_source_store_usable() {
        let source = std::env::temp_dir().join(format!(
            "zhfd-mig-fail-src-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let destination = std::env::temp_dir().join(format!(
            "zhfd-mig-fail-dst-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&source);
        let _ = fs::remove_dir_all(&destination);
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&destination).unwrap();
        let mut source_config = Config::default();
        source_config.runtime.stable_frames = 4;
        fs::write(
            source.join("config.toml"),
            toml::to_string_pretty(&source_config).unwrap(),
        )
        .unwrap();
        // A destination that already owns a config must never be overwritten.
        fs::write(destination.join("config.toml"), "# 用户已有配置\n").unwrap();

        let store = audit_store(&source, source_config);
        let error = store.migrate_to_root(&destination).unwrap_err();
        assert!(
            error.to_string().contains("已有 config.toml"),
            "迁移必须明确拒绝覆盖目标配置: {error:#}"
        );
        assert_eq!(store.root, source, "迁移失败后必须继续使用源 root");
        assert!(source.join("config.toml").is_file());
        assert_eq!(
            fs::read_to_string(destination.join("config.toml")).unwrap(),
            "# 用户已有配置\n",
            "迁移失败不得改写目标配置"
        );
        // A refused migration must not have written anything into the
        // destination either: the rejection happens before any copy.
        let mut entries: Vec<_> = fs::read_dir(&destination)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        entries.sort();
        assert_eq!(
            entries,
            vec!["config.toml".to_string()],
            "被拒绝的迁移不得在目标目录留下任何复制内容: {entries:?}"
        );
        let _ = fs::remove_dir_all(&source);
        let _ = fs::remove_dir_all(&destination);
    }

    #[test]
    fn alternate_config_load_does_not_touch_the_current_file() {
        let root = std::env::temp_dir().join(format!(
            "zhfd-alternate-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        let directory = root.join("other");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("config.toml");
        let mut config = Config::default();
        config.emulator.adb_path = "tools/adb.exe".into();
        fs::write(&path, toml::to_string_pretty(&config).unwrap()).unwrap();

        let store = ConfigStore::load_alternate(&path).unwrap();
        assert_eq!(store.root, directory);
        assert_eq!(store.config.emulator.adb_path, "tools/adb.exe");
        assert_eq!(store.origin, ConfigOrigin::Explicit);
        let _ = fs::remove_dir_all(&root);
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
