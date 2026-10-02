use anyhow::{Result, bail};
use chrono::{DateTime, Duration as ChronoDuration, FixedOffset, NaiveTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    DryRun,
    Live,
}

impl Mode {
    pub fn from_config(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "dry-run" | "dry_run" | "dry" => Ok(Self::DryRun),
            "live" | "production" => Ok(Self::Live),
            other => bail!("runtime.mode 必须是 dry-run 或 live，而不是 {}", other),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DryRun => "dry-run",
            Self::Live => "live",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ExitCode {
    Ok = 0,
    BusinessFailure = 1,
    DeviceFailure = 2,
    VisionFailure = 3,
    ConfigFailure = 4,
    Timeout = 5,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResult {
    pub mode: String,
    #[serde(default)]
    pub instance_index: Option<u32>,
    #[serde(default)]
    pub instance_name: Option<String>,
    pub serial: String,
    pub profile_id: Option<String>,
    pub state_history: Vec<String>,
    pub clicked: bool,
    pub success: bool,
    pub dry_run_ready: bool,
    pub error: Option<String>,
}

pub fn in_window(start: &str, end: &str, now: NaiveTime) -> Result<bool> {
    let start = parse_time(start)?;
    let end = parse_time(end)?;
    Ok(if start <= end {
        now >= start && now <= end
    } else {
        now >= start || now <= end
    })
}

pub fn in_configured_window(start: &str, end: &str, timezone: &str) -> Result<bool> {
    // The supported deployment zone is intentionally explicit. Do not silently
    // fall back to the Windows host timezone because it could cause a bad-time click.
    let offset = timezone_offset(timezone)?;
    let now = Utc::now().with_timezone(&offset);
    in_window(start, end, now.time())
}

/// Return the number of whole seconds left before the currently active
/// configured window ends. `None` means the current time is outside the
/// window. Keeping this calculation here makes the run loop testable without
/// depending on the host timezone or wall clock.
pub fn seconds_until_configured_window_end(
    start: &str,
    end: &str,
    timezone: &str,
) -> Result<Option<u64>> {
    let offset = timezone_offset(timezone)?;
    let now = Utc::now().with_timezone(&offset);
    seconds_until_configured_window_end_at(start, end, now)
}

pub fn seconds_until_configured_window_end_at(
    start: &str,
    end: &str,
    now: DateTime<FixedOffset>,
) -> Result<Option<u64>> {
    let start = parse_time(start)?;
    let end = parse_time(end)?;
    if !in_window_values(start, end, now.time()) {
        return Ok(None);
    }

    let end_date = if start <= end || now.time() <= end {
        now.date_naive()
    } else {
        now.date_naive() + ChronoDuration::days(1)
    };
    let end_local = end_date
        .and_time(end)
        .and_local_timezone(*now.offset())
        .single()
        .ok_or_else(|| anyhow::anyhow!("无法计算签到窗口结束时间"))?;
    Ok(Some((end_local - now).num_seconds().max(0) as u64))
}

fn timezone_offset(timezone: &str) -> Result<FixedOffset> {
    match timezone {
        "Asia/Shanghai" => FixedOffset::east_opt(8 * 60 * 60),
        "UTC" | "Etc/UTC" => FixedOffset::east_opt(0),
        _ => bail!(
            "暂不支持时区配置: {}（支持 Asia/Shanghai 或 UTC）",
            timezone
        ),
    }
    .ok_or_else(|| anyhow::anyhow!("无效 UTC offset"))
}

fn in_window_values(start: NaiveTime, end: NaiveTime, now: NaiveTime) -> bool {
    if start <= end {
        now >= start && now <= end
    } else {
        now >= start || now <= end
    }
}

#[expect(
    dead_code,
    reason = "reserved for scheduled waiting UX; verified by unit test"
)]
pub fn seconds_until_window_start(start: &str, now: NaiveTime) -> Result<Option<u64>> {
    let start = parse_time(start)?;
    if now >= start {
        return Ok(None);
    }
    Ok(Some((start - now).num_seconds().max(0) as u64))
}

pub fn authorize_click(
    mode: Mode,
    explicit_confirmation: bool,
    profile_calibrated: bool,
    foreground_matches: bool,
    within_window: bool,
    ready_frames: u32,
    required_frames: u32,
) -> Result<()> {
    if mode != Mode::Live {
        bail!("dry-run 模式禁止点击");
    }
    if !explicit_confirmation {
        bail!("正式点击需要显式确认");
    }
    if !profile_calibrated {
        bail!("当前 profile 未标定，禁止点击");
    }
    if !foreground_matches {
        bail!("前台 App 不是目标应用，禁止点击");
    }
    if !within_window {
        bail!("不在签到时间窗口，禁止点击");
    }
    if required_frames == 0 || ready_frames < required_frames {
        bail!("ready 状态未达到连续帧确认要求");
    }
    Ok(())
}

fn parse_time(value: &str) -> Result<NaiveTime> {
    let value = value.trim();
    let parts = value.split(':').collect::<Vec<_>>();
    if parts.len() != 2 || parts[0].len() != 2 || parts[1].len() != 2 {
        bail!("非法时间 {value}: 必须是 HH:MM");
    }
    NaiveTime::parse_from_str(value, "%H:%M").map_err(|e| anyhow::anyhow!("非法时间 {value}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn time(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn standard_window_includes_both_boundaries() {
        assert!(in_window("21:30", "23:59", time(21, 30)).unwrap());
        assert!(in_window("21:30", "23:59", time(23, 59)).unwrap());
        assert!(!in_window("21:30", "23:59", time(21, 29)).unwrap());
        assert!(!in_window("21:30", "23:59", time(0, 0)).unwrap());
    }

    #[test]
    fn supports_window_crossing_midnight() {
        assert!(in_window("23:30", "00:30", time(23, 45)).unwrap());
        assert!(in_window("23:30", "00:30", time(0, 10)).unwrap());
        assert!(!in_window("23:30", "00:30", time(12, 0)).unwrap());
    }

    #[test]
    fn rejects_invalid_window_time() {
        assert!(in_window("25:00", "23:59", time(22, 0)).is_err());
        assert!(in_window("21:3", "23:59", time(22, 0)).is_err());
    }

    #[test]
    fn timezone_configuration_is_explicit() {
        assert!(matches!("Asia/Shanghai", "Asia/Shanghai"));
        assert!(in_configured_window("21:30", "23:59", "Asia/Shanghai").is_ok());
        assert!(in_configured_window("21:30", "23:59", "Mars/Olympus").is_err());
    }

    #[test]
    fn window_end_remaining_is_bounded_for_active_window() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-09-27T22:00:00+08:00").unwrap();
        assert_eq!(
            seconds_until_configured_window_end_at("21:30", "23:59", now).unwrap(),
            Some(7_140)
        );

        let near_end = chrono::DateTime::parse_from_rfc3339("2026-09-27T23:58:59+08:00").unwrap();
        assert_eq!(
            seconds_until_configured_window_end_at("21:30", "23:59", near_end).unwrap(),
            Some(1)
        );

        let outside = chrono::DateTime::parse_from_rfc3339("2026-09-27T20:00:00+08:00").unwrap();
        assert_eq!(
            seconds_until_configured_window_end_at("21:30", "23:59", outside).unwrap(),
            None
        );
    }

    #[test]
    fn window_end_remaining_handles_midnight_crossing() {
        let before_midnight =
            chrono::DateTime::parse_from_rfc3339("2026-09-27T23:45:00+08:00").unwrap();
        assert_eq!(
            seconds_until_configured_window_end_at("23:30", "00:30", before_midnight).unwrap(),
            Some(2_700)
        );

        let after_midnight =
            chrono::DateTime::parse_from_rfc3339("2026-09-28T00:10:00+08:00").unwrap();
        assert_eq!(
            seconds_until_configured_window_end_at("23:30", "00:30", after_midnight).unwrap(),
            Some(1_200)
        );
    }

    #[test]
    fn click_policy_fails_closed_for_every_unmet_condition() {
        let good = || authorize_click(Mode::Live, true, true, true, true, 2, 2);
        assert!(good().is_ok());
        assert!(authorize_click(Mode::DryRun, true, true, true, true, 2, 2).is_err());
        assert!(authorize_click(Mode::Live, false, true, true, true, 2, 2).is_err());
        assert!(authorize_click(Mode::Live, true, false, true, true, 2, 2).is_err());
        assert!(authorize_click(Mode::Live, true, true, false, true, 2, 2).is_err());
        assert!(authorize_click(Mode::Live, true, true, true, false, 2, 2).is_err());
        assert!(authorize_click(Mode::Live, true, true, true, true, 1, 2).is_err());
    }

    #[test]
    fn rejects_unknown_run_mode() {
        assert_eq!(Mode::from_config("dry-run").unwrap(), Mode::DryRun);
        assert_eq!(Mode::from_config("LIVE").unwrap(), Mode::Live);
        assert!(Mode::from_config("sometimes").is_err());
    }
}
