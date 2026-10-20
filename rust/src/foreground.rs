//! Foreground-package observation and checks (FR-013).
//!
//! A failed query and a different foreground package are different facts and
//! must stay distinguishable. This module is shared by the run state machine
//! and the login feature; it depends only on `adb`, the run log, and the result
//! type.

use crate::adb;
use crate::domain::{RunResult, set_run_failure};
use crate::logger::RunLog;
use anyhow::Result;

/// Record the foreground package and keep the last trustworthy value in the
/// result. A failed query is a distinct fact from "another package is in the
/// foreground" and must not silently clear the last known package (FR-013).
pub(crate) fn log_foreground_snapshot(
    device: &adb::AdbDevice,
    log: &RunLog,
    stage: &str,
    result: &mut RunResult,
) -> Result<Option<String>> {
    match device.current_package() {
        Ok(package) => {
            result.foreground_package = Some(package.clone());
            log.event(
                "foreground_snapshot",
                serde_json::json!({"stage": stage, "package": package}),
            )?;
            Ok(Some(package))
        }
        Err(error) => {
            // A failed query is recorded as such and is *not* treated as "the
            // App is still in the foreground" (requirement FR-013).
            log.event(
                "foreground_snapshot",
                serde_json::json!({"stage": stage, "error": format!("{error:#}")}),
            )?;
            Ok(None)
        }
    }
}

/// Classify a foreground snapshot against the expected package.
///
/// `None` means the App is confirmed to be in the foreground; otherwise this
/// returns the stable code plus the recovery text. A failed query and a
/// different package are deliberately different outcomes (requirement FR-013).
pub(crate) fn foreground_check(
    snapshot: Option<&str>,
    package_name: &str,
) -> Option<(&'static str, &'static str)> {
    match snapshot {
        Some(package) if package == package_name => None,
        Some(_) => Some((
            "target_app_not_foreground",
            "目标 App 已离开前台；请将其普通签到页置于前台后重新运行，程序未执行点击。",
        )),
        None => Some((
            "foreground_query_failed",
            "无法读取前台应用；出于安全考虑已停止，程序未执行点击。",
        )),
    }
}

/// Fail the run when the target App is not (or can no longer be confirmed as)
/// the foreground package, keeping the two causes distinguishable.
pub(crate) fn require_target_foreground(
    device: &adb::AdbDevice,
    log: &RunLog,
    stage: &str,
    result: &mut RunResult,
    package_name: &str,
) -> Result<bool> {
    let snapshot = log_foreground_snapshot(device, log, stage, result)?;
    match foreground_check(snapshot.as_deref(), package_name) {
        None => Ok(true),
        Some((code, recovery)) => {
            set_run_failure(result, code, stage, recovery);
            Ok(false)
        }
    }
}
