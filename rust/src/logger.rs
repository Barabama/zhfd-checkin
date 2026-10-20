use crate::config::{Config, ConfigStore};
use crate::domain::RunResult;
use anyhow::{Context, Result, bail};
use chrono::Local;
use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

pub struct RunLog {
    pub dir: PathBuf,
    events: PathBuf,
    /// Monotonic per-run counter backing `next_observation_id`. A hierarchy
    /// read must carry a unique id across the whole run, not just within one
    /// navigation stage (requirement FR-011).
    observations: AtomicUsize,
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

/// Identifier for a batch run. It is a timestamp plus pid, matching the log
/// directory naming so a batch id and a run id are visually comparable, but it
/// deliberately does NOT create a log directory: the batch only owns
/// `logs/batch-<id>/summary.json`, and per-instance results keep their own
/// directories.
pub fn new_batch_id() -> String {
    format!(
        "{}{}",
        Local::now().format("%Y-%m-%d_%H%M%S_%3f"),
        std::process::id()
    )
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
            observations: AtomicUsize::new(0),
        })
    }

    /// Next unique observation id for a hierarchy read in this run.
    pub fn next_observation_id(&self) -> usize {
        self.observations.fetch_add(1, Ordering::Relaxed) + 1
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

/// Write the run-level summary for a batch (`--all-instances`) execution.
///
/// The summary lives in its own `logs/batch-<id>/` directory so concurrent
/// batches can never overwrite each other's aggregate result. Per-instance
/// `result.json` files stay in their own log directories and are referenced
/// from the summary entries.
pub fn write_batch_summary(
    root: &Path,
    batch_id: &str,
    summary: &serde_json::Value,
) -> Result<PathBuf> {
    let dir = root.join("logs").join(format!("batch-{batch_id}"));
    fs::create_dir_all(&dir).with_context(|| format!("无法创建批量摘要目录: {}", dir.display()))?;
    let path = dir.join("summary.json");
    fs::write(&path, serde_json::to_vec_pretty(summary)?)
        .with_context(|| format!("写入批量运行摘要失败: {}", path.display()))?;
    Ok(path)
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

/// One-line rendering of a structured event for the GUI timeline.
///
/// The v0.3 event vocabulary is what ties a route decision to the same round's
/// foreground read, hierarchy dump and screenshot (requirement FR-012), so each
/// kind renders the fields that carry that linkage.
fn event_summary(kind: &str, data: &serde_json::Value) -> String {
    let field = |name: &str| data.get(name).map(value_summary).unwrap_or_default();
    match kind {
        "hierarchy_observation" => format!(
            "#{} stage={} status={} bytes={} utf8={} markers={}",
            field("observation_id"),
            field("stage"),
            field("status"),
            field("bytes"),
            field("utf8_valid"),
            field("marker_summary")
        ),
        "hierarchy_unavailable" => format!(
            "#{} stage={} reason={} exit={}",
            field("observation_id"),
            field("stage"),
            field("reason"),
            field("exit_code")
        ),
        "route_markers_missing" => format!(
            "#{} stage={} markers={}",
            field("observation_id"),
            field("stage"),
            field("marker_summary")
        ),
        "foreground_snapshot" => format!(
            "stage={} package={} error={}",
            field("stage"),
            field("package"),
            field("error")
        ),
        "page_route" => format!("stage={} route={}", field("stage"), field("route")),
        "page_observation" => format!("route={} mode={}", field("route"), field("checkin_mode")),
        "navigation_action" => format!(
            "action={} label={} stage={} before={}",
            field("action"),
            field("label"),
            field("stage"),
            field("before_route")
        ),
        "state" => format!(
            "button={} coverage={}",
            field("state"),
            field("color_coverage")
        ),
        "locating_wait" => format!(
            "elapsed={}s limit={}s clicked=false",
            field("elapsed_seconds"),
            field("limit_seconds")
        ),
        "blocked" => format!("blocked_by={}", field("blocked_by")),
        "foreign_window" => format!(
            "#{} stage={} package={} nodes={} focus={}",
            field("observation_id"),
            field("stage"),
            field("package"),
            field("node_count"),
            field("focused_window")
        ),
        "reobserve" => format!(
            "stage={} attempt={} status={} overlay={}",
            field("stage"),
            field("attempt"),
            field("status"),
            field("overlay_package")
        ),
        "navigation_blocked" => format!(
            "reason={} overlay={}",
            field("reason"),
            field("overlay_package")
        ),
        "home_scroll" => format!("attempt={}", field("attempt")),
        "clicked_ready" | "tap" => format!("x={} y={}", field("x"), field("y")),
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
        let batch_path = dir.join("summary.json");
        if batch_path.is_file() {
            // Run-level summary for a batch (`--all-instances`). It carries the
            // shared blocker and one entry per planned instance (D-05/D-07), so
            // it must be visible next to the per-instance results.
            let value: serde_json::Value = match fs::read_to_string(&batch_path)
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
            {
                Some(value) => value,
                None => continue,
            };
            let status = value
                .get("status")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown");
            let instances = value
                .get("instances")
                .and_then(serde_json::Value::as_array)
                .map(|entries| entries.len())
                .unwrap_or(0);
            let blocked_by = value
                .get("blocked_by")
                .and_then(serde_json::Value::as_str)
                .map(|value| format!(" blocked_by={value}"))
                .unwrap_or_default();
            summaries.push(LogSummary {
                directory: dir,
                kind: "batch".into(),
                status: status.into(),
                summary: format!(
                    "batch / mode={} / status={}{} / instances={}",
                    value
                        .get("mode")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("unknown"),
                    status,
                    blocked_by,
                    instances
                ),
            });
        } else if sync_path.is_file() {
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
            } else if result.blocked_by.is_some() {
                // A blocked instance is not an ordinary failure: the batch never
                // started it, so its status must read as blocked (D-05). The
                // blocker itself is rendered in the summary line below.
                "blocked"
            } else {
                result
                    .error_code
                    .as_deref()
                    .or(result.error.as_deref())
                    .unwrap_or("incomplete")
            };
            let instance = result
                .instance_index
                .map(|index| format!("instance={}", index))
                .unwrap_or_else(|| "instance=unknown".into());
            let blocked = result
                .blocked_by
                .as_deref()
                .map(|value| format!(" blocked_by={value}"))
                .unwrap_or_default();
            summaries.push(LogSummary {
                directory: dir,
                kind: "run".into(),
                status: status.into(),
                summary: format!(
                    "run / {} / {} / {}{} / route={} mode={} state={} clicked={} success={}",
                    instance,
                    profile,
                    status,
                    blocked,
                    result
                        .route
                        .as_deref()
                        .or_else(|| result.page_route.map(crate::domain::PageRoute::as_str))
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
    ["result.json", "summary.json", "sync.json", "events.jsonl"]
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
    if !["result.json", "summary.json", "sync.json", "events.jsonl"].contains(&artifact_name) {
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
            auto_login_attempted: None,
            auto_login_ready: None,
            auto_login_succeeded: None,
            auto_login_method: None,
            auto_login_account_configured: None,
            auto_login_account_matched: None,
            auto_login_tap_count: None,
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
            hierarchy_status: None,
            overlay_package: None,
            app_version: Some("0.3.0".into()),
            git_commit: Some("test".into()),
            git_dirty: Some(false),
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
            auto_login_attempted: None,
            auto_login_ready: None,
            auto_login_succeeded: None,
            auto_login_method: None,
            auto_login_account_configured: None,
            auto_login_account_matched: None,
            auto_login_tap_count: None,
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
            hierarchy_status: None,
            overlay_package: None,
            app_version: None,
            git_commit: None,
            git_dirty: None,
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
            auto_login_attempted: None,
            auto_login_ready: None,
            auto_login_succeeded: None,
            auto_login_method: None,
            auto_login_account_configured: None,
            auto_login_account_matched: None,
            auto_login_tap_count: None,
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
            hierarchy_status: None,
            overlay_package: None,
            app_version: None,
            git_commit: None,
            git_dirty: None,
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
    fn recent_logs_reports_batch_summaries_as_their_own_kind() {
        let root = std::env::temp_dir().join(format!(
            "zhfd-batch-logs-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        let batch = root.join("logs/batch-2026-10-14_221000_000_1");
        fs::create_dir_all(&batch).unwrap();
        fs::write(
            batch.join("summary.json"),
            serde_json::json!({
                "schema_version": 3,
                "mode": "dry-run",
                "status": "blocked",
                "blocked_by": "shared_preflight",
                "error_code": "shared_preflight_failed",
                "instances": [
                    {"index": 0, "status": "blocked"},
                    {"index": 1, "status": "blocked"},
                ]
            })
            .to_string(),
        )
        .unwrap();

        let logs = recent_logs(&root, 10).unwrap();
        let entry = logs
            .iter()
            .find(|entry| entry.kind == "batch")
            .expect("批量摘要必须出现在日志列表中");
        assert_eq!(entry.status, "blocked");
        assert!(entry.summary.contains("blocked_by=shared_preflight"));
        assert!(entry.summary.contains("instances=2"));
        // The summary must also be previewable/exportable from the logs page.
        let artifact = log_artifact_path(&entry.directory);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(
            artifact.and_then(|path| path.file_name().map(|name| name.to_owned())),
            Some("summary.json".into())
        );
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
