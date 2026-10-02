use crate::config::{Config, ConfigStore};
use crate::domain::RunResult;
use anyhow::{Context, Result, bail};
use chrono::Local;
use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

pub struct RunLog {
    pub dir: PathBuf,
    events: PathBuf,
}

pub struct SyncLog {
    pub dir: PathBuf,
    events: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncGeometry {
    pub source: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub density_dpi: Option<u32>,
    pub profile_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncResult {
    pub action: String,
    pub profile_id: String,
    pub instance_index: u32,
    pub target: SyncGeometry,
    pub before: Option<SyncGeometry>,
    pub after: Option<SyncGeometry>,
    pub backup_path: Option<String>,
    pub mutation_attempted: bool,
    pub modified: bool,
    pub restarted: bool,
    pub rollback_attempted: bool,
    pub rollback_succeeded: bool,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LogSummary {
    pub directory: PathBuf,
    pub kind: String,
    pub status: String,
    pub summary: String,
}

#[derive(Debug, Serialize)]
struct Event<'a, T: Serialize> {
    at: String,
    kind: &'a str,
    data: T,
}

fn unique_log_dir(root: &Path, prefix: &str) -> Result<PathBuf> {
    fs::create_dir_all(root.join("logs"))?;
    let timestamp = Local::now().format("%Y-%m-%d_%H%M%S_%3f").to_string();
    let pid = std::process::id();
    for attempt in 0..1000u32 {
        let suffix = if attempt == 0 {
            format!("{}{}_{}", prefix, timestamp, pid)
        } else {
            format!("{}{}_{}_{}", prefix, timestamp, pid, attempt)
        };
        let dir = root.join("logs").join(suffix);
        match fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    bail!("无法创建唯一日志目录")
}

impl SyncLog {
    pub fn new(root: &Path) -> Result<Self> {
        let dir = unique_log_dir(root, "sync-")?;
        Ok(Self {
            events: dir.join("events.jsonl"),
            dir,
        })
    }

    pub fn event<T: Serialize>(&self, kind: &str, data: T) -> Result<()> {
        let line = serde_json::to_string(&Event {
            at: Local::now().to_rfc3339(),
            kind,
            data,
        })?;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.events)?;
        writeln!(f, "{}", line)?;
        println!("{}", line);
        Ok(())
    }

    pub fn save_json<T: Serialize>(&self, name: &str, value: &T) -> Result<PathBuf> {
        let p = self.dir.join(name);
        fs::write(&p, serde_json::to_vec_pretty(value)?)?;
        Ok(p)
    }
}

impl RunLog {
    pub fn new(root: &Path) -> Result<Self> {
        let dir = unique_log_dir(root, "")?;
        fs::create_dir(dir.join("screenshots"))?;
        Ok(Self {
            events: dir.join("events.jsonl"),
            dir,
        })
    }
    pub fn event<T: Serialize>(&self, kind: &str, data: T) -> Result<()> {
        let line = serde_json::to_string(&Event {
            at: Local::now().to_rfc3339(),
            kind,
            data,
        })?;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.events)?;
        writeln!(f, "{}", line)?;
        println!("{}", line);
        Ok(())
    }
    pub fn save_bytes(&self, name: &str, bytes: &[u8]) -> Result<PathBuf> {
        let p = self.dir.join("screenshots").join(name);
        fs::write(&p, bytes).with_context(|| format!("保存截图失败: {}", p.display()))?;
        Ok(p)
    }
    pub fn save_json<T: Serialize>(&self, name: &str, value: &T) -> Result<PathBuf> {
        let p = self.dir.join(name);
        fs::write(&p, serde_json::to_vec_pretty(value)?)?;
        Ok(p)
    }
}

#[derive(Debug, Serialize)]
pub struct DiagnosticReport {
    pub generated_at: String,
    pub config_path: String,
    pub config: Config,
    pub latest_log_dir: Option<String>,
    pub latest_result: Option<RunResult>,
}

pub fn write_diagnostic_report(store: &ConfigStore) -> Result<PathBuf> {
    let (latest_log_dir, latest_result) = match latest_result(&store.root)? {
        Some((dir, result)) => (Some(dir.display().to_string()), Some(result)),
        None => (None, None),
    };
    let reports = store.root.join("reports");
    fs::create_dir_all(&reports)?;
    let path = reports.join(format!(
        "diagnostic-{}.json",
        Local::now().format("%Y%m%d-%H%M%S")
    ));
    let mut config = store.config.clone();
    // Reports are intended for sharing and must not contain notification
    // endpoints or future credential-bearing config fields.
    config.notifications.bark_url.clear();
    let report = DiagnosticReport {
        generated_at: Local::now().to_rfc3339(),
        config_path: store.path.display().to_string(),
        config,
        latest_log_dir,
        latest_result,
    };
    fs::write(&path, serde_json::to_vec_pretty(&report)?)
        .with_context(|| format!("写入诊断报告失败: {}", path.display()))?;
    Ok(path)
}

pub fn recent_logs(root: &Path, limit: usize) -> Result<Vec<LogSummary>> {
    let logs = root.join("logs");
    if !logs.is_dir() || limit == 0 {
        return Ok(Vec::new());
    }
    let mut dirs: Vec<_> = fs::read_dir(&logs)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort_by(|a, b| b.file_name().cmp(&a.file_name()));

    let mut summaries = Vec::new();
    for dir in dirs.into_iter().take(limit) {
        let sync_path = dir.join("sync.json");
        let run_path = dir.join("result.json");
        if sync_path.is_file() {
            let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(&sync_path)?)?;
            let profile = value
                .get("profile_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown");
            let status = value
                .get("status")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown");
            let rollback = value
                .get("rollback_succeeded")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let summary = if rollback {
                format!("sync / {} / {} / 已回滚", profile, status)
            } else {
                format!("sync / {} / {}", profile, status)
            };
            summaries.push(LogSummary {
                directory: dir,
                kind: "sync".into(),
                status: status.into(),
                summary,
            });
        } else if run_path.is_file() {
            let result: RunResult = serde_json::from_str(&fs::read_to_string(&run_path)?)?;
            let profile = result.profile_id.as_deref().unwrap_or("unknown");
            let status = if result.success {
                "success"
            } else if result.dry_run_ready {
                "dry-run-ready"
            } else {
                result.error.as_deref().unwrap_or("incomplete")
            };
            let instance = result
                .instance_index
                .map(|index| format!("instance={}", index))
                .unwrap_or_else(|| "instance=unknown".into());
            summaries.push(LogSummary {
                directory: dir,
                kind: "run".into(),
                status: status.into(),
                summary: format!(
                    "run / {} / {} / {} / clicked={} success={}",
                    instance, profile, status, result.clicked, result.success
                ),
            });
        }
    }
    Ok(summaries)
}

pub fn latest_result(root: &Path) -> Result<Option<(PathBuf, RunResult)>> {
    let logs = root.join("logs");
    if !logs.is_dir() {
        return Ok(None);
    }
    let mut dirs: Vec<_> = fs::read_dir(&logs)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    for dir in dirs {
        let result_path = dir.join("result.json");
        if !result_path.is_file() {
            continue;
        }
        let text = fs::read_to_string(&result_path)
            .with_context(|| format!("读取运行结果失败: {}", result_path.display()))?;
        let result: RunResult = serde_json::from_str(&text)
            .with_context(|| format!("解析运行结果失败: {}", result_path.display()))?;
        return Ok(Some((dir, result)));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::fs;

    #[test]
    fn run_and_sync_logs_get_distinct_directories_when_created_together() {
        let root = std::env::temp_dir().join(format!("zhfd-log-collision-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let run = RunLog::new(&root).unwrap();
        let sync = SyncLog::new(&root).unwrap();
        assert_ne!(run.dir, sync.dir);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn latest_result_reads_newest_timestamped_log() {
        let root = std::env::temp_dir().join(format!("zhfd-logger-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("logs/2026-01-01_000000")).unwrap();
        fs::create_dir_all(root.join("logs/2026-01-02_000000")).unwrap();
        let result = RunResult {
            mode: "dry-run".into(),
            instance_index: None,
            instance_name: None,
            serial: "serial".into(),
            profile_id: Some("profile".into()),
            state_history: vec!["gray".into()],
            clicked: false,
            success: false,
            dry_run_ready: false,
            error: Some("location_timeout".into()),
        };
        fs::write(
            root.join("logs/2026-01-02_000000/result.json"),
            serde_json::to_vec(&result).unwrap(),
        )
        .unwrap();
        let (dir, loaded) = latest_result(&root).unwrap().unwrap();
        assert!(dir.ends_with("2026-01-02_000000"));
        assert_eq!(loaded.error.as_deref(), Some("location_timeout"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn recent_logs_reads_run_and_sync_entries() {
        let root = std::env::temp_dir().join(format!("zhfd-recent-logs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("logs/2026-09-27_010000")).unwrap();
        fs::create_dir_all(root.join("logs/sync-2026-09-27_020000")).unwrap();
        let result = RunResult {
            mode: "dry-run".into(),
            instance_index: None,
            instance_name: None,
            serial: "serial".into(),
            profile_id: Some("profile".into()),
            state_history: vec!["ready".into()],
            clicked: false,
            success: false,
            dry_run_ready: true,
            error: None,
        };
        fs::write(
            root.join("logs/2026-09-27_010000/result.json"),
            serde_json::to_vec(&result).unwrap(),
        )
        .unwrap();
        fs::write(
            root.join("logs/sync-2026-09-27_020000/sync.json"),
            serde_json::json!({
                "profile_id": "landscape_1600x900_d240",
                "status": "success",
                "rollback_succeeded": false
            })
            .to_string(),
        )
        .unwrap();
        let logs = recent_logs(&root, 10).unwrap();
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].kind, "sync");
        assert_eq!(logs[1].status, "dry-run-ready");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn diagnostic_report_redacts_notification_url() {
        let root = std::env::temp_dir().join(format!("zhfd-report-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("config.toml");
        let config = Config::default();
        let store = ConfigStore {
            root: root.clone(),
            path,
            config,
        };
        // This test only verifies the report's redaction through a serialized
        // report generated by the same public function.
        let mut store = store;
        store.config.notifications.bark_url = "https://example.invalid/secret".into();
        let report_path = write_diagnostic_report(&store).unwrap();
        let text = fs::read_to_string(report_path).unwrap();
        assert!(!text.contains("secret"));
        let _ = fs::remove_dir_all(&root);
    }
}
