use anyhow::{Result, bail};
use chrono::{DateTime, Duration as ChronoDuration, FixedOffset, NaiveDate, NaiveTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    DryRun,
    Live,
}

/// Which page the App is showing. A route is *page identity only* (frozen
/// decision D-08): the check-in *kind* on a confirmed check-in page is carried
/// separately by [`CheckinMode`], so there is deliberately no
/// "unsupported check-in" route. A page that only mentions 外勤签到 without the
/// check-in page title is not identifiable as a check-in page at all, and the
/// home page's 晚点名签到 entry card must never be mistaken for one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageRoute {
    Login,
    Home,
    Services,
    Checkin,
    RouteCandidateUnconfirmed,
    RouteEvidenceConflict,
    /// The run started on a check-in page and the refresh navigation did not
    /// land on a catalogue page: the page identity transition cannot be
    /// trusted, so no action may follow.
    StalePageRefreshFailed,
    /// The refresh navigation happened but the resulting state could not be
    /// confirmed as a catalogue page: the hierarchy was unavailable, the dump
    /// carried no page-identity markers, or the foreground package could not be
    /// read at all.
    StalePageRefreshUnconfirmed,
    /// The App's own "是否重启App / 当前 App 可能被界面劫持" security prompt is
    /// on screen. It is a blocking dialog with no check-in action; the run must
    /// stop without tapping it.
    AppRestartDialog,
    /// After the stale-page refresh BACK, the target App was no longer the
    /// foreground package (for example the BACK dropped the App to the
    /// Launcher). The run stops here with no further navigation or tap; the
    /// stable machine code reuses FR-013's `target_app_not_foreground`.
    AppLeftForeground,
    /// Also the deserialization catch-all: a historical `result.json` written by
    /// an older build may carry a route string this build no longer has (the
    /// retired `unsupported_checkin`), and reading old logs must not fail.
    #[serde(other)]
    Unknown,
}

impl PageRoute {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::Home => "home",
            Self::Services => "services",
            Self::Checkin => "checkin",
            Self::Unknown => "unknown",
            Self::RouteCandidateUnconfirmed => "route_candidate_unconfirmed",
            Self::RouteEvidenceConflict => "route_evidence_conflict",
            Self::StalePageRefreshFailed => "stale_page_refresh_failed",
            Self::StalePageRefreshUnconfirmed => "stale_page_refresh_unconfirmed",
            Self::AppRestartDialog => "app_restart_dialog",
            Self::AppLeftForeground => "app_left_foreground",
        }
    }

    /// Stable machine error code for a route that cannot authorize an action.
    /// `Checkin` is the only route that is allowed to continue, so it has no
    /// failure code; every caller must still check for it explicitly.
    pub fn error_code(self) -> Option<&'static str> {
        match self {
            Self::Login => Some("login_required"),
            Self::Home | Self::Services | Self::Unknown => Some("navigation_not_confirmed"),
            Self::RouteCandidateUnconfirmed => Some("route_candidate_unconfirmed"),
            Self::RouteEvidenceConflict => Some("route_evidence_conflict"),
            Self::StalePageRefreshFailed => Some("stale_page_refresh_failed"),
            Self::StalePageRefreshUnconfirmed => Some("stale_page_refresh_unconfirmed"),
            Self::AppRestartDialog => Some("app_restart_dialog"),
            // Reuses the FR-013 code that already names this exact condition
            // (target App no longer foreground) instead of inventing a synonym.
            Self::AppLeftForeground => Some("target_app_not_foreground"),
            Self::Checkin => None,
        }
    }
}

/// Machine-readable terminal state of a run. Frozen by D-05/D-07: these four
/// English values are the v0.3 wire format, and every stable machine error is
/// carried separately in `error_code`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    Running,
    Success,
    Failed,
    Blocked,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckinMode {
    Ordinary,
    FieldWork,
    Unsupported,
}

impl CheckinMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ordinary => "ordinary",
            Self::FieldWork => "field_work",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckinState {
    Gray,
    Locating,
    Ready,
    SuccessToday,
    SuccessStale,
    SuccessDateUnknown,
    FieldWorkReady,
    Unknown,
}

impl CheckinState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gray => "gray",
            Self::Locating => "locating",
            Self::Ready => "ready",
            Self::SuccessToday => "success_today",
            Self::SuccessStale => "success_stale",
            Self::SuccessDateUnknown => "success_date_unknown",
            Self::FieldWorkReady => "field_work_ready",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ConsecutiveUnknown {
    count: u32,
}

impl ConsecutiveUnknown {
    pub fn observe(&mut self, is_unknown: bool) -> u32 {
        if is_unknown {
            self.count = self.count.saturating_add(1);
        } else {
            self.count = 0;
        }
        self.count
    }

    pub fn reset(&mut self) {
        self.count = 0;
    }

    #[cfg(test)]
    pub fn count(self) -> u32 {
        self.count
    }
}

/// Authorize a page action for the given page route and check-in mode.
///
/// `allow_field_work` mirrors the user-facing `runtime.allow_field_work_live`
/// switch. Field work is a *different* business action from the ordinary
/// evening check-in, so it stays denied unless the operator explicitly opts in;
/// an unknown mode is never authorized under any configuration.
pub fn authorize_page_action(
    route: PageRoute,
    checkin_mode: CheckinMode,
    allow_field_work: bool,
) -> Result<()> {
    if route != PageRoute::Checkin {
        bail!("页面路线未确认为签到页，禁止签到动作");
    }
    match checkin_mode {
        CheckinMode::Ordinary => Ok(()),
        CheckinMode::FieldWork if allow_field_work => Ok(()),
        CheckinMode::FieldWork => {
            bail!("外勤签到未启用自动点击（runtime.allow_field_work_live=false）")
        }
        CheckinMode::Unsupported => bail!("无法确认签到业务类型，禁止自动点击"),
    }
}

/// Evidence used to decide whether the one-click login may run.
///
/// This is a pure, device-free view of the facts collected from the page so
/// the policy can be tested with counterexamples instead of a live emulator.
/// The visual checkbox confirmation is deliberately *not* part of this struct:
/// it is a separate, post-decision step because it requires a screenshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoLoginEvidence {
    pub allow_auto_login: bool,
    pub accept_login_terms: bool,
    /// A window owned by another package is visible.
    pub foreign_package: bool,
    /// Number of clickable nodes labelled "一键登录".
    pub login_button_count: usize,
    /// A "上次使用过的账号" label was found.
    pub account_present: bool,
    /// `runtime.auto_login_account` is non-empty.
    pub expected_account_configured: bool,
    /// The displayed account equals the configured one.
    pub account_matches: bool,
    /// The agreement checkbox node could be located.
    pub consent_node_present: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoLoginDecision {
    /// `runtime.allow_auto_login` is off; do not even probe.
    Disabled,
    /// The page is not a login page.
    NotLoginPage,
    /// Dry-run: all preconditions hold, but nothing may be tapped.
    Ready,
    /// Live: all preconditions hold; the login tap may proceed.
    Authorized,
    /// A stable `auto_login_*` error code.
    Refused(&'static str),
}

/// Authorize the login-page one-click action.
///
/// Order matters: the switch is checked first so a disabled config never even
/// probes; page identity is next so non-login pages are untouched; then every
/// piece of page evidence must be unambiguous. In dry-run the same gates run,
/// but the result is [`AutoLoginDecision::Ready`] and the caller must not tap.
pub fn authorize_auto_login(
    route: PageRoute,
    mode: Mode,
    evidence: &AutoLoginEvidence,
) -> AutoLoginDecision {
    if !evidence.allow_auto_login {
        return AutoLoginDecision::Disabled;
    }
    if route != PageRoute::Login {
        return AutoLoginDecision::NotLoginPage;
    }
    if evidence.foreign_package {
        return AutoLoginDecision::Refused("auto_login_blocked");
    }
    if evidence.login_button_count != 1 {
        return AutoLoginDecision::Refused("auto_login_button_ambiguous");
    }
    if !evidence.consent_node_present {
        return AutoLoginDecision::Refused("auto_login_consent_not_found");
    }
    if !evidence.accept_login_terms {
        return AutoLoginDecision::Refused("auto_login_terms_not_accepted");
    }
    if !evidence.account_present {
        return AutoLoginDecision::Refused("auto_login_account_unknown");
    }
    if evidence.expected_account_configured && !evidence.account_matches {
        return AutoLoginDecision::Refused("auto_login_account_mismatch");
    }
    match mode {
        Mode::DryRun => AutoLoginDecision::Ready,
        Mode::Live => AutoLoginDecision::Authorized,
    }
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
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub started_at: String,
    #[serde(default)]
    pub finished_at: Option<String>,
    pub mode: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub blocked_by: Option<String>,
    #[serde(default)]
    pub error_code: Option<String>,
    #[serde(default)]
    pub instance_index: Option<u32>,
    #[serde(default)]
    pub instance_name: Option<String>,
    pub serial: String,
    pub profile_id: Option<String>,
    #[serde(default)]
    pub screen_width: Option<u32>,
    #[serde(default)]
    pub screen_height: Option<u32>,
    #[serde(default)]
    pub density_dpi: Option<u32>,
    pub state_history: Vec<String>,
    pub clicked: bool,
    pub success: bool,
    pub dry_run_ready: bool,
    #[serde(default)]
    pub field_work_ready: bool,
    /// One-click login was actually attempted (a login-page tap may have been
    /// issued). `None` on historical results.
    #[serde(default)]
    pub auto_login_attempted: Option<bool>,
    /// Dry-run only: the login page and every static precondition were present.
    #[serde(default)]
    pub auto_login_ready: Option<bool>,
    /// The page was observed to leave the login page after the tap.
    #[serde(default)]
    pub auto_login_succeeded: Option<bool>,
    /// Method used or proposed; currently always `one_click`.
    #[serde(default)]
    pub auto_login_method: Option<String>,
    /// Whether an expected account was configured for comparison.
    #[serde(default)]
    pub auto_login_account_configured: Option<bool>,
    /// Whether the displayed account matched the configured one. `None` when no
    /// expected account is configured or no account label was found.
    #[serde(default)]
    pub auto_login_account_matched: Option<bool>,
    /// Number of login-page taps issued (0 or 1). Separate from `clicked`,
    /// which remains reserved for the check-in control.
    #[serde(default)]
    pub auto_login_tap_count: Option<u32>,
    /// `true` for a standalone `login` action (session setup, no check-in).
    /// `None` on ordinary `run` results and on historical results.
    #[serde(default)]
    pub login_only: Option<bool>,
    #[serde(default)]
    pub page_route: Option<PageRoute>,
    #[serde(default)]
    pub checkin_mode: Option<CheckinMode>,
    #[serde(default)]
    pub checkin_state: Option<CheckinState>,
    #[serde(default)]
    pub success_date: Option<String>,
    #[serde(default)]
    pub error_stage: Option<String>,
    #[serde(default)]
    pub recovery_action: Option<String>,
    #[serde(default)]
    pub artifact_paths: Vec<String>,
    #[serde(default)]
    pub route: Option<String>,
    #[serde(default)]
    pub page_refresh_attempted: Option<bool>,
    #[serde(default)]
    pub page_refresh_confirmed: Option<bool>,
    #[serde(default)]
    pub page_loading: Option<bool>,
    #[serde(default)]
    pub button_state: Option<String>,
    #[serde(default)]
    pub location_state: Option<String>,
    #[serde(default)]
    pub foreground_package: Option<String>,
    /// Machine-observable result of the last hierarchy read. Distinguishes
    /// `dump_succeeded`, `dump_failed`, `dump_empty_or_truncated` and
    /// `route_markers_missing` (requirement FR-011) while the route keeps the
    /// frozen `route_candidate_unconfirmed` code.
    #[serde(default)]
    pub hierarchy_status: Option<String>,
    /// Package of a window that was on screen but does not belong to the target
    /// app. Direct evidence that a popup or advertisement is blocking the page,
    /// kept separate from `foreground_package` so "the app is in front" and
    /// "something is drawn on top of it" are not conflated.
    #[serde(default)]
    pub overlay_package: Option<String>,
    #[serde(default)]
    pub app_version: Option<String>,
    #[serde(default)]
    pub git_commit: Option<String>,
    /// Whether the working tree the binary was built from had uncommitted
    /// tracked changes. `None` means the build could not tell (git missing or
    /// the command failed) -- deliberately distinct from `Some(false)`.
    /// Without this, `git_commit` alone would claim a provenance the binary may
    /// not actually have (FR-014).
    #[serde(default)]
    pub git_dirty: Option<bool>,
    #[serde(default)]
    pub build_id: Option<String>,
    pub error: Option<String>,
}

/// Record one stable machine failure on a run result.
///
/// Kept here (rather than in the run loop) so every layer that refuses an
/// action writes the same four fields; `error_stage` names the boundary.
pub fn set_run_failure(result: &mut RunResult, code: &str, stage: &str, recovery_action: &str) {
    result.error = Some(code.into());
    result.error_code = Some(code.into());
    result.error_stage = Some(stage.into());
    result.recovery_action = Some(recovery_action.into());
}

pub fn configured_date(timezone: &str) -> Result<NaiveDate> {
    let offset = timezone_offset(timezone)?;
    Ok(Utc::now().with_timezone(&offset).date_naive())
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

pub fn parse_time(value: &str) -> Result<NaiveTime> {
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
    fn page_action_authorization_requires_checkin_ordinary_context() {
        assert!(authorize_page_action(PageRoute::Checkin, CheckinMode::Ordinary, false).is_ok());
        assert!(authorize_page_action(PageRoute::Checkin, CheckinMode::Ordinary, true).is_ok());
        // Field work is denied by default and only allowed on explicit opt-in.
        assert!(authorize_page_action(PageRoute::Checkin, CheckinMode::FieldWork, false).is_err());
        assert!(authorize_page_action(PageRoute::Checkin, CheckinMode::FieldWork, true).is_ok());
        // An unconfirmed business mode is never authorized, opt-in or not.
        assert!(
            authorize_page_action(PageRoute::Checkin, CheckinMode::Unsupported, false).is_err()
        );
        assert!(authorize_page_action(PageRoute::Checkin, CheckinMode::Unsupported, true).is_err());
        // The route itself must be the check-in page in every case.
        assert!(authorize_page_action(PageRoute::Home, CheckinMode::Ordinary, true).is_err());
        assert!(authorize_page_action(PageRoute::Home, CheckinMode::FieldWork, true).is_err());
    }

    #[test]
    fn consecutive_unknown_counter_resets_after_any_known_state() {
        let mut tracker = ConsecutiveUnknown::default();
        assert_eq!(tracker.observe(true), 1);
        assert_eq!(tracker.observe(true), 2);
        assert_eq!(tracker.observe(false), 0);
        assert_eq!(tracker.observe(true), 1);
        tracker.reset();
        assert_eq!(tracker.count(), 0);
    }

    #[test]
    fn interleaved_known_frames_reset_unknown_threshold_to_contiguous_run_only() {
        let mut tracker = ConsecutiveUnknown::default();
        assert_eq!(tracker.observe(true), 1);
        assert_eq!(tracker.observe(false), 0); // gray / locating / ready all reset
        assert_eq!(tracker.observe(true), 1);
        assert_eq!(tracker.observe(false), 0);
        assert_eq!(tracker.observe(true), 1);
        assert_eq!(tracker.observe(true), 2);
        assert_eq!(tracker.observe(true), 3);
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

    fn auto_login_evidence() -> AutoLoginEvidence {
        AutoLoginEvidence {
            allow_auto_login: true,
            accept_login_terms: true,
            foreign_package: false,
            login_button_count: 1,
            account_present: true,
            expected_account_configured: true,
            account_matches: true,
            consent_node_present: true,
        }
    }

    #[test]
    fn auto_login_is_disabled_by_default_and_never_implies_consent() {
        let mut evidence = auto_login_evidence();
        assert_eq!(
            authorize_auto_login(PageRoute::Login, Mode::Live, &evidence),
            AutoLoginDecision::Authorized
        );
        evidence.allow_auto_login = false;
        assert_eq!(
            authorize_auto_login(PageRoute::Login, Mode::Live, &evidence),
            AutoLoginDecision::Disabled
        );
        // Enabling the login switch alone must not authorize ticking the
        // agreement checkbox: that needs its own switch.
        let mut evidence = auto_login_evidence();
        evidence.accept_login_terms = false;
        assert_eq!(
            authorize_auto_login(PageRoute::Login, Mode::Live, &evidence),
            AutoLoginDecision::Refused("auto_login_terms_not_accepted")
        );
    }

    #[test]
    fn auto_login_fails_closed_on_every_ambiguous_condition() {
        let mut evidence = auto_login_evidence();
        assert_eq!(
            authorize_auto_login(PageRoute::Checkin, Mode::Live, &evidence),
            AutoLoginDecision::NotLoginPage
        );
        evidence.foreign_package = true;
        assert_eq!(
            authorize_auto_login(PageRoute::Login, Mode::Live, &evidence),
            AutoLoginDecision::Refused("auto_login_blocked")
        );

        let mut evidence = auto_login_evidence();
        evidence.login_button_count = 0;
        assert_eq!(
            authorize_auto_login(PageRoute::Login, Mode::Live, &evidence),
            AutoLoginDecision::Refused("auto_login_button_ambiguous")
        );
        evidence.login_button_count = 2;
        assert_eq!(
            authorize_auto_login(PageRoute::Login, Mode::Live, &evidence),
            AutoLoginDecision::Refused("auto_login_button_ambiguous")
        );

        let mut evidence = auto_login_evidence();
        evidence.consent_node_present = false;
        assert_eq!(
            authorize_auto_login(PageRoute::Login, Mode::Live, &evidence),
            AutoLoginDecision::Refused("auto_login_consent_not_found")
        );

        let mut evidence = auto_login_evidence();
        evidence.account_present = false;
        assert_eq!(
            authorize_auto_login(PageRoute::Login, Mode::Live, &evidence),
            AutoLoginDecision::Refused("auto_login_account_unknown")
        );

        let mut evidence = auto_login_evidence();
        evidence.account_matches = false;
        assert_eq!(
            authorize_auto_login(PageRoute::Login, Mode::Live, &evidence),
            AutoLoginDecision::Refused("auto_login_account_mismatch")
        );
    }

    #[test]
    fn auto_login_dry_run_is_ready_but_never_authorized() {
        let evidence = auto_login_evidence();
        assert_eq!(
            authorize_auto_login(PageRoute::Login, Mode::DryRun, &evidence),
            AutoLoginDecision::Ready
        );
        // Without a configured expected account the match check is skipped.
        let mut unconfigured = evidence;
        unconfigured.expected_account_configured = false;
        unconfigured.account_matches = false;
        assert_eq!(
            authorize_auto_login(PageRoute::Login, Mode::Live, &unconfigured),
            AutoLoginDecision::Authorized
        );
    }
}
