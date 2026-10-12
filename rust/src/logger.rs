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

#[derive(Debug, Clone)]
pub struct EventSummary {
    pub at: String,
    pub kind: String,
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

pub fn read_recent_events(dir: &Path, limit: usize) -> Result<Vec<EventSummary>> {
    let path = dir.join("events.jsonl");
    if !path.is_file() || limit == 0 {
        return Ok(Vec::new());
    }
    let mut events = Vec::new();
    for line in fs::read_to_string(&path)?.lines() {
        let value: serde_json::Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let at = value
            .get("at")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let kind = value
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let data = value
            .get("data")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let summary = event_summary(&kind, &data);
        events.push(EventSummary { at, kind, summary });
    }
    let start = events.len().saturating_sub(limit);
    Ok(events.into_iter().skip(start).collect())
}

fn event_summary(kind: &str, data: &serde_json::Value) -> String {
    let field = |name: &str| data.get(name).map(value_summary).unwrap_or_default();
    match kind {
        "hierarchy_observation" => format!(
            "stage={} status={} bytes={} markers={}",
            field("stage"),
            field("status"),
            field("bytes"),
            field("marker_summary")
        ),
        "foreground_snapshot" => format!(
            "stage={} package={} error={}",
            field("stage"),
            field("package"),
            field("error")
        ),
        "page_route" => format!("route={}", field("route")),
        "state" => format!(
            "button={} coverage={}",
            field("state"),
            field("color_coverage")
        ),
        "home_scroll" => format!("attempt={}", field("attempt")),
        "clicked_ready" => format!("x={} y={}", field("x"), field("y")),
        _ => serde_json::to_string(data).unwrap_or_else(|_| "{}".into()),
    }
}

fn value_summary(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Null => "unknown".into(),
        other => other.to_string(),
    }
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
            // A single truncated or partially written artifact must not hide the
            // whole log list; skip unreadable entries and keep the rest.
            let value: serde_json::Value = match fs::read_to_string(&sync_path)
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
            {
                Some(value) => value,
                None => continue,
            };
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
            let result: RunResult = match fs::read_to_string(&run_path)
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
            {
                Some(result) => result,
                None => continue,
            };
            let profile = result.profile_id.as_deref().unwrap_or("unknown");
            let status = if result.success {
                "success"
            } else if result.field_work_ready {
                "field_work_ready"
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
                    "run / {} / {} / {} / route={} mode={} state={} clicked={} success={}",
                    instance,
                    profile,
                    status,
                    result
                        .page_route
                        .map(|route| route.as_str())
                        .unwrap_or("unknown"),
                    result
                        .checkin_mode
                        .map(|mode| mode.as_str())
                        .unwrap_or("unknown"),
                    result
                        .checkin_state
                        .map(|state| state.as_str())
                        .unwrap_or("unknown"),
                    result.clicked,
                    result.success
                ),
            });
        }
    }
    Ok(summaries)
}

pub fn log_artifact_path(directory: &Path) -> Option<PathBuf> {
    ["result.json", "sync.json", "events.jsonl"]
        .into_iter()
        .map(|name| directory.join(name))
        .find(|path| path.is_file())
}

pub fn read_log_artifact(directory: &Path) -> Result<(PathBuf, String)> {
    let path = log_artifact_path(directory)
        .ok_or_else(|| anyhow::anyhow!("日志目录没有可展示的结果文件: {}", directory.display()))?;
    let text = fs::read_to_string(&path)
        .with_context(|| format!("读取日志证据失败: {}", path.display()))?;
    let display = if path.file_name().is_some_and(|name| name == "events.jsonl") {
        text
    } else {
        let json: serde_json::Value = serde_json::from_str(&text)
            .with_context(|| format!("解析日志证据失败: {}", path.display()))?;
        serde_json::to_string_pretty(&json)?
    };
    Ok((path, display))
}

pub fn export_log_artifact(directory: &Path, destination: &Path) -> Result<PathBuf> {
    let source = log_artifact_path(directory)
        .ok_or_else(|| anyhow::anyhow!("日志目录没有可导出的结果文件: {}", directory.display()))?;
    copy_log_artifact(&source, destination)
}

pub fn export_log_artifact_named(
    directory: &Path,
    artifact_name: &str,
    destination: &Path,
) -> Result<PathBuf> {
    if !["result.json", "sync.json", "events.jsonl"].contains(&artifact_name) {
        bail!("不支持导出的日志证据类型: {artifact_name}");
    }
    let source = directory.join(artifact_name);
    if !source.is_file() {
        bail!("日志证据不存在: {}", source.display());
    }
    copy_log_artifact(&source, destination)
}

fn copy_log_artifact(source: &Path, destination: &Path) -> Result<PathBuf> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("无法创建导出目录: {}", parent.display()))?;
    }
    fs::copy(source, destination).with_context(|| {
        format!(
            "导出日志证据失败: {} -> {}",
            source.display(),
            destination.display()
        )
    })?;
    Ok(destination.to_path_buf())
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

    // ===== v0.3.0 independent audit counterexamples =====

    #[test]
    fn audit_v02_result_json_remains_readable() {
        let root = std::env::temp_dir().join(format!(
            "zhfd-audit-v02-log-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        let dir = root.join("logs/2026-10-11_221046_211_7940");
        fs::create_dir_all(&dir).unwrap();
        // Representative v0.2 result.json (contains fields removed in v0.3).
        let v02 = serde_json::json!({
            "schema_version": 2,
            "run_id": "2026-10-11_221046_211_7940",
            "started_at": "2026-10-11T22:10:46+08:00",
            "finished_at": "2026-10-11T22:10:56+08:00",
            "mode": "dry-run",
            "instance_index": 1,
            "instance_name": "签到实例 1",
            "serial": "127.0.0.1:5557",
            "profile_id": "portrait_900x1600_d320",
            "screen_width": 900,
            "screen_height": 1600,
            "density_dpi": 320,
            "state_history": ["ready", "ready"],
            "clicked": false,
            "success": false,
            "dry_run_ready": true,
            "field_work_ready": false,
            "page_route": "checkin",
            "checkin_mode": "ordinary",
            "checkin_state": "ready",
            "success_date": null,
            "error_stage": null,
            "recovery_action": null,
            "artifact_paths": ["D:\\logs\\x\\result.json"],
            "error": null
        });
        fs::write(
            dir.join("result.json"),
            serde_json::to_vec_pretty(&v02).unwrap(),
        )
        .unwrap();
        let loaded = latest_result(&root).unwrap();
        let _ = fs::remove_dir_all(&root);
        let (_, result) = loaded.expect("v0.2 result.json 无法读取");
        assert!(result.dry_run_ready);
        assert_eq!(result.serial, "127.0.0.1:5557");
    }

    #[test]
    fn audit_one_corrupt_result_json_must_not_hide_all_logs() {
        let root = std::env::temp_dir().join(format!(
            "zhfd-audit-corrupt-log-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        let good = root.join("logs/2026-10-11_010000_000_1");
        let bad = root.join("logs/2026-10-11_020000_000_1");
        fs::create_dir_all(&good).unwrap();
        fs::create_dir_all(&bad).unwrap();
        let result = RunResult {
            schema_version: 3,
            run_id: "good".into(),
            started_at: String::new(),
            finished_at: None,
            mode: "dry-run".into(),
            status: Some("failed".into()),
            blocked_by: None,
            error_code: Some("navigation_not_confirmed".into()),
            instance_index: Some(0),
            instance_name: Some("one".into()),
            serial: "127.0.0.1:5555".into(),
            profile_id: None,
            screen_width: None,
            screen_height: None,
            density_dpi: None,
            state_history: vec![],
            clicked: false,
            success: false,
            dry_run_ready: false,
            field_work_ready: false,
            page_route: Some(crate::domain::PageRoute::Unknown),
            checkin_mode: Some(crate::domain::CheckinMode::Unsupported),
            checkin_state: Some(crate::domain::CheckinState::Unknown),
            success_date: None,
            error_stage: Some("navigation".into()),
            recovery_action: None,
            artifact_paths: Vec::new(),
            route: Some("unknown".into()),
            page_refresh_attempted: None,
            page_refresh_confirmed: None,

            page_loading: None,
            button_state: None,
            location_state: None,
            foreground_package: None,
            app_version: Some("0.3.0".into()),
            git_commit: Some("test".into()),
            build_id: Some("0.3.0-test".into()),
            error: Some("navigation_not_confirmed".into()),
        };
        fs::write(
            good.join("result.json"),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
        // A truncated / partially written result.json (e.g. process killed mid-write).
        fs::write(bad.join("result.json"), b"{\"schema_version\":3,").unwrap();
        let logs = recent_logs(&root, 10);
        let _ = fs::remove_dir_all(&root);
        match logs {
            Ok(entries) => assert!(
                entries.iter().any(|e| e.summary.contains("instance=0")),
                "有效日志未出现在列表中"
            ),
            Err(error) => panic!("单个损坏 result.json 使整个日志列表失败: {error:#}"),
        }
    }
    #[test]
    fn latest_result_reads_newest_timestamped_log() {
        let root = std::env::temp_dir().join(format!("zhfd-logger-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("logs/2026-01-01_000000")).unwrap();
        fs::create_dir_all(root.join("logs/2026-01-02_000000")).unwrap();
        let result = RunResult {
            schema_version: 2,
            run_id: "test-run".into(),
            started_at: String::new(),
            finished_at: None,
            mode: "dry-run".into(),
            status: None,
            blocked_by: None,
            error_code: None,
            instance_index: None,
            instance_name: None,
            serial: "serial".into(),
            profile_id: Some("profile".into()),
            screen_width: None,
            screen_height: None,
            density_dpi: None,
            state_history: vec!["gray".into()],
            clicked: false,
            success: false,
            dry_run_ready: false,
            field_work_ready: false,
            page_route: None,
            checkin_mode: None,
            checkin_state: None,
            success_date: None,
            error_stage: None,
            recovery_action: None,
            artifact_paths: Vec::new(),
            route: None,
            page_refresh_attempted: None,
            page_refresh_confirmed: None,

            page_loading: None,
            button_state: None,
            location_state: None,
            foreground_package: None,
            app_version: None,
            git_commit: None,
            build_id: None,
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
            schema_version: 2,
            run_id: "test-run".into(),
            started_at: String::new(),
            finished_at: None,
            mode: "dry-run".into(),
            status: None,
            blocked_by: None,
            error_code: None,
            instance_index: None,
            instance_name: None,
            serial: "serial".into(),
            profile_id: Some("profile".into()),
            screen_width: None,
            screen_height: None,
            density_dpi: None,
            state_history: vec!["ready".into()],
            clicked: false,
            success: false,
            dry_run_ready: true,
            field_work_ready: false,
            page_route: Some(crate::domain::PageRoute::Checkin),
            checkin_mode: Some(crate::domain::CheckinMode::Ordinary),
            checkin_state: Some(crate::domain::CheckinState::Ready),
            success_date: None,
            error_stage: None,
            recovery_action: None,
            artifact_paths: Vec::new(),
            route: Some("checkin".into()),
            page_refresh_attempted: Some(false),
            page_refresh_confirmed: Some(false),

            page_loading: Some(false),
            button_state: Some("ready".into()),
            location_state: Some("ready".into()),
            foreground_package: None,
            app_version: None,
            git_commit: None,
            build_id: None,
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
    fn named_export_can_select_events_even_when_result_json_exists() {
        let root = std::env::temp_dir().join(format!(
            "zhfd-export-events-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let log_dir = root.join("logs/run");
        fs::create_dir_all(&log_dir).unwrap();
        fs::write(log_dir.join("result.json"), b"result").unwrap();
        fs::write(log_dir.join("events.jsonl"), b"event-line\n").unwrap();
        let destination = root.join("export/events.jsonl");
        export_log_artifact_named(&log_dir, "events.jsonl", &destination).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"event-line\n");
        assert!(export_log_artifact_named(&log_dir, "..\\config.toml", &destination).is_err());
        let _ = fs::remove_dir_all(root);
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
            origin: crate::config::ConfigOrigin::Explicit,
            created_default: false,
            recovery_error: None,
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
