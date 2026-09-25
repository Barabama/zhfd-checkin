use crate::config::{Config, ConfigStore};
use crate::domain::RunResult;
use anyhow::{Context, Result};
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

#[derive(Debug, Serialize)]
struct Event<'a, T: Serialize> {
    at: String,
    kind: &'a str,
    data: T,
}

impl RunLog {
    pub fn new(root: &Path) -> Result<Self> {
        let dir = root
            .join("logs")
            .join(Local::now().format("%Y-%m-%d_%H%M%S").to_string());
        fs::create_dir_all(dir.join("screenshots"))?;
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
    fn latest_result_reads_newest_timestamped_log() {
        let root = std::env::temp_dir().join(format!("zhfd-logger-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("logs/2026-01-01_000000")).unwrap();
        fs::create_dir_all(root.join("logs/2026-01-02_000000")).unwrap();
        let result = RunResult {
            mode: "dry-run".into(),
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
