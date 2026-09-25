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
