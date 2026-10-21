//! Login-page one-click support.
//!
//! This module owns the login-page DOM selectors and the single-tap login
//! flow. It deliberately does **not** read [`crate::profile::Profile`]
//! geometry: every tap target comes from the current hierarchy's node bounds,
//! so the flow adapts to every LDPlayer preset resolution/DPI. Profile
//! calibration still gates *live* runs before this module is reached.

use crate::adb;
use crate::config::{ConfigStore, RuntimeConfig};
use crate::domain::{
    AutoLoginDecision, AutoLoginEvidence, ExitCode, Mode, PageRoute, RunResult,
    authorize_auto_login, set_run_failure,
};
use crate::foreground::log_foreground_snapshot;
use crate::logger::RunLog;
use crate::page::{PageNode, PageObservation, PageView, observe_page_route};
use crate::vision;
use anyhow::Result;
use std::{
    thread,
    time::{Duration, Instant},
};

impl PageView {
    /// Clickable nodes whose own label is exactly `一键登录`.
    fn login_buttons(&self) -> Vec<&PageNode> {
        self.nodes
            .iter()
            .filter(|node| self.in_scope(node) && node.clickable)
            .filter(|node| has_exact_label(node, LOGIN_BUTTON_LABEL))
            .collect()
    }

    fn consent_text_node(&self) -> Option<&PageNode> {
        self.nodes
            .iter()
            .filter(|node| self.in_scope(node))
            .find(|node| node.labels.contains(CONSENT_TEXT_LABEL))
    }

    /// The agreement checkbox has no label or `checked` attribute. It is the
    /// single small, clickable, label-less node sitting immediately left of the
    /// consent sentence in the same vertical band.
    ///
    /// Size limits are expressed relative to the consent text, not in absolute
    /// pixels, so the finder works at every LDPlayer preset resolution/DPI.
    fn consent_checkbox(&self) -> Option<&PageNode> {
        let consent = self.consent_text_node()?;
        let candidates: Vec<_> = self
            .nodes
            .iter()
            .filter(|node| self.in_scope(node) && node.clickable)
            .filter(|node| node.labels.trim().is_empty())
            .filter(|node| node.bounds.width().saturating_mul(2) <= consent.bounds.width())
            .filter(|node| node.bounds.height() <= consent.bounds.height().saturating_mul(2))
            .filter(|node| node.bounds.vertical_overlap(consent.bounds))
            .filter(|node| node.bounds.right <= consent.bounds.left)
            .collect();
        match candidates.as_slice() {
            [only] => Some(*only),
            _ => None,
        }
    }

    /// The App's remembered account shown above the login button. Returns
    /// `None` when the label is missing or carries more than one distinct value;
    /// callers must treat that as "account unknown" and fail closed.
    fn last_used_account(&self) -> Option<String> {
        let mut accounts: Vec<String> = self
            .nodes
            .iter()
            .filter(|node| self.in_scope(node))
            .filter_map(|node| account_from_label(&node.labels))
            .collect();
        accounts.sort();
        accounts.dedup();
        match accounts.as_slice() {
            [only] => Some(only.clone()),
            _ => None,
        }
    }
}

const LOGIN_BUTTON_LABEL: &str = "一键登录";
const CONSENT_TEXT_LABEL: &str = "登录即代表您已同意";
const LAST_USED_ACCOUNT_LABEL: &str = "上次使用过的账号";

fn has_exact_label(node: &PageNode, label: &str) -> bool {
    node.labels.lines().any(|line| line.trim() == label)
}

fn account_from_label(labels: &str) -> Option<String> {
    let position = labels.find(LAST_USED_ACCOUNT_LABEL)?;
    let head = labels[..position]
        .split('\n')
        .next()
        .unwrap_or("")
        .trim()
        .trim_end_matches("&#10;")
        .trim();
    let length = head.chars().count();
    if (4..=32).contains(&length) && !head.chars().any(char::is_whitespace) {
        Some(head.to_string())
    } else {
        None
    }
}

/// Outcome of one login-page attempt.
///
/// Both callers (the `run` state machine and the standalone `login` command)
/// interpret the same evidence; keeping the outcome explicit avoids
/// duplicating the gate chain or the tap sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoginAttempt {
    /// `runtime.allow_auto_login` is off; nothing was probed.
    Disabled,
    /// The App is already past the login page (Home/Services/Checkin).
    AlreadyLoggedIn,
    /// The page is neither the login page nor a known post-login page.
    NotLoginPage,
    /// dry-run: every gate passed, but nothing was tapped.
    Ready,
    /// The single login tap was issued and the page left the login page.
    Succeeded,
    /// Refused with a stable `auto_login_*` code; the result carries it.
    Refused(&'static str),
}

/// Optional login-page step for the `run` state machine.
///
/// Returns `Ok(Some(code))` when the run must stop, and `Ok(None)` when the
/// caller should continue into the normal navigation/check-in flow.
pub(crate) fn auto_login_if_required(
    device: &adb::AdbDevice,
    log: &RunLog,
    result: &mut RunResult,
    mode: Mode,
    store: &ConfigStore,
    target_package: &str,
) -> Result<Option<ExitCode>> {
    let runtime = &store.config.runtime;
    if !runtime.allow_auto_login {
        // Feature disabled: keep the historical behavior and cost exactly zero
        // extra hierarchy dumps.
        return Ok(None);
    }
    match attempt_login(device, log, result, mode, runtime, target_package)? {
        LoginAttempt::Disabled
        | LoginAttempt::AlreadyLoggedIn
        | LoginAttempt::NotLoginPage
        | LoginAttempt::Ready
        | LoginAttempt::Succeeded => Ok(None),
        LoginAttempt::Refused(code) => {
            if mode == Mode::DryRun {
                // Dry-run is observational: record the refusal but leave the
                // historical `login_required` outcome to the navigation step.
                Ok(None)
            } else {
                Ok(Some(auto_login_exit_code(code)))
            }
        }
    }
}

/// One-click login as a standalone action (`zhfd-checkin login`).
///
/// This never navigates and never touches the check-in control: it is a
/// session-setup action, not a check-in action. Profile calibration is not
/// required because every tap target comes from the hierarchy's own bounds.
///
/// `mode` is `Live` only after the caller received the explicit
/// `--confirm-login` acknowledgement; without it the same gates run in
/// `DryRun` and nothing is tapped.
pub(crate) fn login_only(
    device: &adb::AdbDevice,
    log: &RunLog,
    result: &mut RunResult,
    store: &ConfigStore,
    mode: Mode,
    target_package: &str,
) -> Result<LoginAttempt> {
    let runtime = &store.config.runtime;
    if !runtime.allow_auto_login {
        return Ok(LoginAttempt::Disabled);
    }
    attempt_login(device, log, result, mode, runtime, target_package)
}

/// Probe the page, run every login gate, and (only when authorized) tap once.
fn attempt_login(
    device: &adb::AdbDevice,
    log: &RunLog,
    result: &mut RunResult,
    mode: Mode,
    runtime: &RuntimeConfig,
    target_package: &str,
) -> Result<LoginAttempt> {
    result.auto_login_attempted = Some(false);
    result.auto_login_tap_count = Some(0);

    let observation = observe_page_route(
        device,
        log,
        "auto_login_probe",
        log.next_observation_id(),
        target_package,
    )?;
    if observation.route != PageRoute::Login {
        // A post-login page means there is nothing to do; anything else is a
        // page the login action cannot act on.
        return Ok(match observation.route {
            PageRoute::Home | PageRoute::Services | PageRoute::Checkin => {
                result.auto_login_ready = Some(false);
                LoginAttempt::AlreadyLoggedIn
            }
            _ => LoginAttempt::NotLoginPage,
        });
    }
    result.auto_login_method = Some("one_click".into());

    let account = observation.view.last_used_account();
    let expected = runtime.auto_login_account.trim();
    let account_configured = !expected.is_empty();
    let account_matches = account.as_deref() == Some(expected);
    result.auto_login_account_configured = Some(account_configured);
    result.auto_login_account_matched = if account_configured {
        Some(account_matches)
    } else {
        None
    };

    let evidence = AutoLoginEvidence {
        allow_auto_login: true,
        accept_login_terms: runtime.accept_login_terms,
        foreign_package: observation.foreign_package.is_some(),
        login_button_count: observation.view.login_buttons().len(),
        account_present: account.is_some(),
        expected_account_configured: account_configured,
        account_matches,
        consent_node_present: observation.view.consent_checkbox().is_some(),
    };
    log.event(
        "auto_login_probe",
        serde_json::json!({
            "route": observation.route.as_str(),
            "overlay_package": observation.foreign_package,
            "login_button_count": evidence.login_button_count,
            "account_present": evidence.account_present,
            "account_configured": evidence.expected_account_configured,
            "account_matched": evidence.account_matches,
            "consent_node_present": evidence.consent_node_present,
            "clicked": false,
        }),
    )?;

    match authorize_auto_login(observation.route, mode, &evidence) {
        AutoLoginDecision::Disabled => Ok(LoginAttempt::Disabled),
        AutoLoginDecision::NotLoginPage => Ok(LoginAttempt::NotLoginPage),
        AutoLoginDecision::Ready => {
            result.auto_login_ready = Some(true);
            log.event(
                "auto_login_ready",
                serde_json::json!({"method": "one_click", "clicked": false}),
            )?;
            Ok(LoginAttempt::Ready)
        }
        AutoLoginDecision::Refused(code) => {
            result.auto_login_ready = Some(false);
            log.event(
                "auto_login_refused",
                serde_json::json!({
                    "reason": code,
                    "mode": mode.as_str(),
                    "clicked": false,
                }),
            )?;
            // Dry-run stays observational; only an authorized mode records the
            // refusal as the run's terminal failure.
            if mode != Mode::DryRun {
                set_run_failure(result, code, "auto_login", auto_login_recovery(code));
            }
            Ok(LoginAttempt::Refused(code))
        }
        AutoLoginDecision::Authorized => {
            perform_one_click_login(device, log, result, &observation, runtime, target_package)
        }
    }
}

/// Execute the one-click login after every static gate has passed.
///
/// The only tap that is not part of the check-in flow happens here. It is
/// deliberately bounded: at most one consent-checkbox tap and one login tap per
/// run, and every later round is observation-only.
fn perform_one_click_login(
    device: &adb::AdbDevice,
    log: &RunLog,
    result: &mut RunResult,
    observation: &PageObservation,
    runtime: &RuntimeConfig,
    target_package: &str,
) -> Result<LoginAttempt> {
    if observation.view.login_buttons().len() != 1 {
        return refuse_auto_login(result, "auto_login_button_ambiguous");
    }
    let Some(checkbox) = observation.view.consent_checkbox() else {
        return refuse_auto_login(result, "auto_login_consent_not_found");
    };
    let checkbox_bounds = checkbox.bounds;
    let checkbox_center = checkbox.center;
    let account_before = observation.view.last_used_account();

    // 1. Read-only confirmation of the current checkbox state.
    let bytes = device.screenshot()?;
    log.save_bytes("auto_login_before.png", &bytes)?;
    let image = vision::decode(&bytes)?;
    let already_checked = vision::consent_checkbox_checked(&image, checkbox_bounds.as_rect());
    if already_checked {
        log.event(
            "auto_login_consent",
            serde_json::json!({"action": "already_checked", "clicked": false}),
        )?;
    } else {
        // 2. Tick the agreement once, then prove it changed before touching the
        // login button. If it cannot be proven, stop without a login tap.
        device.tap(checkbox_center.0, checkbox_center.1)?;
        log.event(
            "auto_login_consent",
            serde_json::json!({
                "action": "tapped",
                "x": checkbox_center.0,
                "y": checkbox_center.1,
                "clicked": true,
            }),
        )?;
        thread::sleep(Duration::from_millis(600));
        let bytes = device.screenshot()?;
        log.save_bytes("auto_login_consent.png", &bytes)?;
        let image = vision::decode(&bytes)?;
        if !vision::consent_checkbox_checked(&image, checkbox_bounds.as_rect()) {
            log.event(
                "auto_login_consent",
                serde_json::json!({"action": "unconfirmed", "clicked": true}),
            )?;
            return refuse_auto_login(result, "auto_login_consent_unconfirmed");
        }
    }

    // 3. Re-observe before the login tap: the page, the account and the single
    // button must all still be the same evidence, mirroring the check-in policy.
    let recheck = observe_page_route(
        device,
        log,
        "auto_login_pre_tap",
        log.next_observation_id(),
        target_package,
    )?;
    if recheck.route != PageRoute::Login || recheck.foreign_package.is_some() {
        return refuse_auto_login(result, "auto_login_not_confirmed");
    }
    let buttons = recheck.view.login_buttons();
    let Some(recheck_checkbox) = recheck.view.consent_checkbox() else {
        return refuse_auto_login(result, "auto_login_not_confirmed");
    };
    if buttons.len() != 1 {
        return refuse_auto_login(result, "auto_login_not_confirmed");
    }
    if recheck.view.last_used_account() != account_before {
        return refuse_auto_login(result, "auto_login_account_mismatch");
    }
    let login_center_now = buttons[0].center;
    let bytes = device.screenshot()?;
    log.save_bytes("auto_login_pre_tap.png", &bytes)?;
    let image = vision::decode(&bytes)?;
    if !vision::consent_checkbox_checked(&image, recheck_checkbox.bounds.as_rect()) {
        return refuse_auto_login(result, "auto_login_consent_unconfirmed");
    }

    // 4. The single login tap of this run.
    result.auto_login_attempted = Some(true);
    result.auto_login_tap_count = Some(1);
    device.tap(login_center_now.0, login_center_now.1)?;
    log.event(
        "auto_login_tap",
        serde_json::json!({
            "method": "one_click",
            "x": login_center_now.0,
            "y": login_center_now.1,
            "clicked": true,
        }),
    )?;

    // 5. Observation-only wait. No further tap is ever issued here.
    let timeout = Duration::from_secs(runtime.auto_login_timeout_seconds.clamp(5, 120));
    let started = Instant::now();
    let mut saw_readable = false;
    // The App can pass through a short loading/transition page after login.
    // Require the same positive route in two consecutive observations before
    // handing control back to navigation, otherwise navigation may observe the
    // half-rendered page and report `route_candidate_unconfirmed`.
    let mut confirmed_route: Option<PageRoute> = None;
    loop {
        thread::sleep(Duration::from_millis(1000));
        match log_foreground_snapshot(device, log, "auto_login_wait", result)? {
            Some(package) if package == target_package => {}
            Some(_) => {
                log.save_bytes("auto_login_blocked.png", &device.screenshot()?)?;
                return refuse_auto_login(result, "auto_login_blocked");
            }
            // A failed query is not "still foreground"; keep observing until
            // the timeout instead of treating it as success.
            None => {}
        }
        let observed = observe_page_route(
            device,
            log,
            "auto_login_wait",
            log.next_observation_id(),
            target_package,
        )?;
        if observed.status == "dump_succeeded" {
            saw_readable = true;
        }
        if observed.foreign_package.is_some() {
            log.save_bytes("auto_login_blocked.png", &device.screenshot()?)?;
            return refuse_auto_login(result, "auto_login_blocked");
        }
        if has_login_dialog(&observed.view) {
            log.save_bytes("auto_login_blocked.png", &device.screenshot()?)?;
            return refuse_auto_login(result, "auto_login_blocked");
        }
        match observed.route {
            route @ (PageRoute::Home | PageRoute::Services | PageRoute::Checkin) => {
                if confirmed_route != Some(route) {
                    confirmed_route = Some(route);
                    log.event(
                        "auto_login_settling",
                        serde_json::json!({"route": route.as_str(), "clicked": true}),
                    )?;
                    continue;
                }
                result.auto_login_succeeded = Some(true);
                log.save_bytes("auto_login_result.png", &device.screenshot()?)?;
                log.event(
                    "auto_login_result",
                    serde_json::json!({
                        "succeeded": true,
                        "route": observed.route.as_str(),
                        "elapsed_ms": started.elapsed().as_millis() as u64,
                        "clicked": true,
                    }),
                )?;
                return Ok(LoginAttempt::Succeeded);
            }
            _ => {
                confirmed_route = None;
            }
        }
        if started.elapsed() >= timeout {
            result.auto_login_succeeded = Some(false);
            log.save_bytes("auto_login_timeout.png", &device.screenshot()?)?;
            let code = if saw_readable {
                "auto_login_timeout"
            } else {
                "auto_login_not_confirmed"
            };
            log.event(
                "auto_login_result",
                serde_json::json!({
                    "succeeded": false,
                    "reason": code,
                    "elapsed_ms": started.elapsed().as_millis() as u64,
                    "clicked": true,
                }),
            )?;
            set_run_failure(result, code, "auto_login", auto_login_recovery(code));
            return Ok(LoginAttempt::Refused(code));
        }
    }
}

fn has_login_dialog(view: &PageView) -> bool {
    ["同意并登录", "请先阅读并同意", "请阅读并同意"]
        .iter()
        .any(|marker| view.scoped_text.contains(marker))
}

fn auto_login_recovery(code: &str) -> &'static str {
    match code {
        "auto_login_terms_not_accepted" => {
            "自动登录未获授权代勾选协议；请在 config.toml 设置 runtime.accept_login_terms = true，或手动登录。"
        }
        "auto_login_consent_not_found" => {
            "登录页未找到协议复选框；页面结构可能已变化，程序未点击登录。请手动登录。"
        }
        "auto_login_consent_unconfirmed" => {
            "点击协议复选框后无法确认已勾选；程序未点击登录。请手动登录。"
        }
        "auto_login_button_ambiguous" => {
            "登录页的“一键登录”按钮缺失或不唯一；程序未点击。请手动登录。"
        }
        "auto_login_account_mismatch" => {
            "登录页显示的上次账号与 runtime.auto_login_account 不一致；程序未点击登录。"
        }
        "auto_login_account_unknown" => {
            "登录页没有可识别的“上次使用过的账号”；程序未点击登录。请手动登录。"
        }
        "auto_login_blocked" => {
            "登录过程中出现外部窗口/弹窗或 App 离开前台；程序已停止，未继续点击。"
        }
        "auto_login_timeout" => {
            "点击一键登录后未在超时内离开登录页；程序已停止，未重试。请手动核对登录状态。"
        }
        "auto_login_not_confirmed" => "登录后页面证据不可读或与预期冲突；程序已停止，未继续签到。",
        _ => "自动登录未完成；程序已停止，未继续签到。",
    }
}

fn auto_login_exit_code(code: &str) -> ExitCode {
    match code {
        "auto_login_timeout" => ExitCode::Timeout,
        "auto_login_not_confirmed" => ExitCode::VisionFailure,
        _ => ExitCode::BusinessFailure,
    }
}

/// Exit code for a stable `auto_login_*` failure code.
pub(crate) fn exit_code_for(code: &str) -> ExitCode {
    auto_login_exit_code(code)
}

fn refuse_auto_login(result: &mut RunResult, code: &'static str) -> Result<LoginAttempt> {
    result.auto_login_succeeded = Some(false);
    set_run_failure(result, code, "auto_login", auto_login_recovery(code));
    Ok(LoginAttempt::Refused(code))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PACKAGE: &str = "cn.edu.fzu.fdxypa";

    fn page(xml: &str) -> PageView {
        PageView::parse(xml, TEST_PACKAGE)
    }

    #[test]
    fn login_page_exposes_one_button_consent_box_and_account() {
        // Sanitized copy of the observed login hierarchy (account replaced by
        // zeros); no real student identifier is stored in the repository.
        let xml = r#"
            <node package="cn.edu.fzu.fdxypa" content-desc="000000000&#10;上次使用过的账号" bounds="[72,136][828,1211]"/>
            <node package="cn.edu.fzu.fdxypa" class="android.widget.Button" content-desc="一键登录" clickable="true" bounds="[72,846][828,952]"/>
            <node package="cn.edu.fzu.fdxypa" content-desc="其他账户登录" clickable="true" bounds="[342,1002][558,1055]"/>
            <node package="cn.edu.fzu.fdxypa" content-desc="" text="" clickable="true" bounds="[82,1116][120,1169]"/>
            <node package="cn.edu.fzu.fdxypa" content-desc="登录即代表您已同意" bounds="[120,1118][398,1180]"/>
            <node package="cn.edu.fzu.fdxypa" content-desc="点击注册" clickable="true" bounds="[383,1485][517,1533]"/>
        "#;
        let view = page(xml);
        let buttons = view.login_buttons();
        assert_eq!(buttons.len(), 1);
        assert_eq!(buttons[0].center, (450, 899));
        assert_eq!(
            view.consent_text_node().map(|node| node.center.1),
            Some(1149)
        );
        assert_eq!(
            view.consent_checkbox().map(|node| node.center),
            Some((101, 1142))
        );
        assert_eq!(view.last_used_account().as_deref(), Some("000000000"));
    }

    #[test]
    fn login_page_helpers_fail_closed_on_ambiguous_or_missing_evidence() {
        let two_buttons = page(
            r#"
            <node package="cn.edu.fzu.fdxypa" content-desc="一键登录" clickable="true" bounds="[72,846][828,952]"/>
            <node package="cn.edu.fzu.fdxypa" content-desc="一键登录" clickable="true" bounds="[72,960][828,1066]"/>
            <node package="cn.edu.fzu.fdxypa" content-desc="登录即代表您已同意" bounds="[120,1118][398,1180]"/>
            <node package="cn.edu.fzu.fdxypa" clickable="true" bounds="[82,1116][120,1169]"/>
            "#,
        );
        assert_eq!(two_buttons.login_buttons().len(), 2);

        let no_account = page(
            r#"
            <node package="cn.edu.fzu.fdxypa" content-desc="一键登录" clickable="true" bounds="[72,846][828,952]"/>
            <node package="cn.edu.fzu.fdxypa" content-desc="登录即代表您已同意" bounds="[120,1118][398,1180]"/>
            <node package="cn.edu.fzu.fdxypa" clickable="true" bounds="[82,1116][120,1169]"/>
            "#,
        );
        assert_eq!(no_account.last_used_account(), None);

        // Two label-less clickable boxes in the consent band: the checkbox is
        // ambiguous, so it must not be returned.
        let ambiguous_box = page(
            r#"
            <node package="cn.edu.fzu.fdxypa" content-desc="登录即代表您已同意" bounds="[120,1118][398,1180]"/>
            <node package="cn.edu.fzu.fdxypa" clickable="true" bounds="[82,1116][120,1169]"/>
            <node package="cn.edu.fzu.fdxypa" clickable="true" bounds="[40,1120][78,1160]"/>
            "#,
        );
        assert!(ambiguous_box.consent_checkbox().is_none());

        // A foreign-package node cannot supply a login button.
        let foreign = page(
            r#"<node package="com.other.app" content-desc="一键登录" clickable="true" bounds="[72,846][828,952]"/>"#,
        );
        assert!(foreign.login_buttons().is_empty());
    }

    #[test]
    fn login_page_helpers_scale_with_every_ldplayer_preset() {
        // The login page finder must not rely on one profile's pixel geometry:
        // scale the observed 900x1600 layout to every preset and require the
        // same single button / checkbox / account to be found.
        let scale = |value: u32, from: u32, to: u32| value * to / from;
        for (width, height, _dpi) in crate::profile::LDPLAYER_PRESETS {
            let sx = |value: u32| scale(value, 900, *width);
            let sy = |value: u32| scale(value, 1600, *height);
            let account_bounds = format!("[{},{}][{},{}]", sx(72), sy(136), sx(828), sy(1211));
            let button_bounds = format!("[{},{}][{},{}]", sx(72), sy(846), sx(828), sy(952));
            let checkbox_bounds = format!("[{},{}][{},{}]", sx(82), sy(1116), sx(120), sy(1169));
            let consent_bounds = format!("[{},{}][{},{}]", sx(120), sy(1118), sx(398), sy(1180));
            let xml = format!(
                r#"
                <node package="cn.edu.fzu.fdxypa" content-desc="000000000&#10;上次使用过的账号" bounds="{account_bounds}"/>
                <node package="cn.edu.fzu.fdxypa" content-desc="一键登录" clickable="true" bounds="{button_bounds}"/>
                <node package="cn.edu.fzu.fdxypa" content-desc="" text="" clickable="true" bounds="{checkbox_bounds}"/>
                <node package="cn.edu.fzu.fdxypa" content-desc="登录即代表您已同意" bounds="{consent_bounds}"/>
                "#
            );
            let view = page(&xml);
            assert_eq!(
                view.login_buttons().len(),
                1,
                "login button at {width}x{height}"
            );
            assert!(
                view.consent_checkbox().is_some(),
                "consent checkbox at {width}x{height}"
            );
            assert_eq!(
                view.last_used_account().as_deref(),
                Some("000000000"),
                "account at {width}x{height}"
            );
        }
    }

    #[test]
    fn login_dialog_markers_are_recognised() {
        assert!(has_login_dialog(&page(r#"<node text="同意并登录"/>"#)));
        assert!(has_login_dialog(&page(r#"<node text="请先阅读并同意"/>"#)));
        assert!(!has_login_dialog(&page(
            r#"<node text="登录即代表您已同意"/>"#
        )));
    }

    #[test]
    fn auto_login_error_codes_map_to_terminal_exit_codes() {
        assert_eq!(
            auto_login_exit_code("auto_login_timeout"),
            ExitCode::Timeout
        );
        assert_eq!(
            auto_login_exit_code("auto_login_not_confirmed"),
            ExitCode::VisionFailure
        );
        assert_eq!(
            auto_login_exit_code("auto_login_blocked"),
            ExitCode::BusinessFailure
        );
    }
}
