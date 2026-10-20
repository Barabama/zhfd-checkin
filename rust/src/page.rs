//! Generic accessibility hierarchy model.
//!
//! This module is the one-way bottom of the page dependency graph: it parses
//! `uiautomator` XML into scoped nodes and geometry, classifies the page route,
//! and observes one hierarchy round. It knows nothing about navigation,
//! check-in, auto-login, or `Profile`. Both the run state machine (`main.rs`)
//! and the login feature (`auto_login.rs`) depend on it.

use crate::adb;
use crate::domain::{CheckinMode, PageRoute};
use crate::logger::RunLog;
use anyhow::Result;

/// Accessibility view of one hierarchy dump, scoped to the target app.
///
/// Page identity must be decided from the *target app's* nodes only. A popup or
/// advertisement rendered by another process can carry the same words and would
/// otherwise be mistaken for the check-in page, which is exactly how a blocking
/// overlay could steer the route decision.
#[derive(Debug, Clone)]
pub(crate) struct PageView {
    target_package: String,
    pub(crate) nodes: Vec<PageNode>,
    /// Distinct `package` attributes in document order.
    pub(crate) packages: Vec<String>,
    /// `text`/`content-desc` of in-scope nodes, joined for marker matching.
    pub(crate) scoped_text: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PageNode {
    /// `package` attribute; `None` when the dump omits it.
    package: Option<String>,
    /// `text` and `content-desc`, newline-joined so a marker cannot be formed
    /// by two adjacent attribute values.
    pub(crate) labels: String,
    pub(crate) clickable: bool,
    pub(crate) center: (u32, u32),
    /// Raw node rectangle; used by geometric finders such as the login-page
    /// agreement checkbox, which has no label of its own.
    pub(crate) bounds: NodeBounds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NodeBounds {
    pub(crate) left: u32,
    pub(crate) top: u32,
    pub(crate) right: u32,
    pub(crate) bottom: u32,
}

impl NodeBounds {
    pub(crate) fn width(self) -> u32 {
        self.right.saturating_sub(self.left)
    }
    pub(crate) fn height(self) -> u32 {
        self.bottom.saturating_sub(self.top)
    }
    pub(crate) fn vertical_overlap(self, other: Self) -> bool {
        self.top <= other.bottom && other.top <= self.bottom
    }
    pub(crate) fn as_rect(self) -> (u32, u32, u32, u32) {
        (self.left, self.top, self.right, self.bottom)
    }
}

impl PageView {
    pub(crate) fn parse(xml: &str, target_package: &str) -> Self {
        let mut nodes = Vec::new();
        let mut packages: Vec<String> = Vec::new();
        let mut scoped_text = String::new();
        // Splitting on `<node` is also the recovery path for a malformed
        // attribute that swallows the start of the next node: the split lands on
        // the nested node, so the rest of the dump is not lost.
        for chunk in xml.split("<node").skip(1) {
            let Some(tag_end) = tag_body_end(chunk, 0) else {
                continue;
            };
            let body = &chunk[..tag_end];
            let package = attribute_value(body, "package")
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            if let Some(package) = &package
                && !packages.iter().any(|existing| existing == package)
            {
                packages.push(package.clone());
            }
            // A node belongs to the page when it declares the target package.
            // Nodes without a `package` attribute (hand-written fixtures and
            // partial dumps) are kept, so scoping can only ever *remove*
            // foreign evidence, never the app's own.
            let in_scope = package
                .as_deref()
                .is_none_or(|package| package == target_package);
            let labels = ["content-desc", "text"]
                .iter()
                .filter_map(|name| attribute_value(body, name))
                .collect::<Vec<_>>()
                .join("\n");
            // Page identity is decided from text alone, so a node without
            // parsable bounds still contributes its markers.
            if in_scope && !labels.is_empty() {
                scoped_text.push_str(&labels);
                scoped_text.push('\n');
            }
            // Only a node with bounds can be a tap target.
            let Some(bounds) = attribute_value(body, "bounds").and_then(bounds_rect) else {
                continue;
            };
            nodes.push(PageNode {
                package,
                labels,
                clickable: attribute_value(body, "clickable") == Some("true"),
                center: (
                    (bounds.left + bounds.right) / 2,
                    (bounds.top + bounds.bottom) / 2,
                ),
                bounds,
            });
        }
        Self {
            target_package: target_package.to_string(),
            nodes,
            packages,
            scoped_text,
        }
    }

    /// A package on the hierarchy root is a window-level owner. Foreign
    /// descendant nodes (for example a system-owned child) are not enough to
    /// prove that a popup is blocking the target app; the focused window is
    /// checked separately by `observe_page_route`.
    pub(crate) fn root_foreign_package(&self) -> Option<&str> {
        self.packages
            .first()
            .map(String::as_str)
            .filter(|package| *package != self.target_package)
    }

    #[cfg(test)]
    pub(crate) fn foreign_package(&self) -> Option<&str> {
        self.root_foreign_package()
    }

    pub(crate) fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub(crate) fn find_labeled(
        &self,
        label: &str,
        min_center_y: u32,
        clickable_only: bool,
    ) -> Option<(u32, u32)> {
        self.nodes
            .iter()
            .filter(|node| {
                node.package
                    .as_deref()
                    .is_none_or(|package| package == self.target_package)
            })
            .filter(|node| !clickable_only || node.clickable)
            .filter(|node| node.labels.contains(label) && node.center.1 >= min_center_y)
            .map(|node| node.center)
            .next()
    }

    pub(crate) fn in_scope(&self, node: &PageNode) -> bool {
        node.package
            .as_deref()
            .is_none_or(|package| package == self.target_package)
    }
}

/// End of a `<node ...>` tag body: the first `>` outside a quoted value.
pub(crate) fn tag_body_end(xml: &str, from: usize) -> Option<usize> {
    let mut in_quote = false;
    for (offset, ch) in xml.get(from..)?.char_indices() {
        match ch {
            '"' => in_quote = !in_quote,
            '>' if !in_quote => return Some(from + offset),
            _ => {}
        }
    }
    None
}

/// Value of `name="..."` inside a tag body.
///
/// The preceding character must be whitespace (or the body start) so an
/// attribute name cannot be matched inside another attribute's value.
pub(crate) fn attribute_value<'a>(body: &'a str, name: &str) -> Option<&'a str> {
    let prefix = format!("{name}=\"");
    let mut cursor = 0usize;
    while let Some(relative) = body[cursor..].find(&prefix) {
        let start = cursor + relative;
        let at_boundary = start == 0
            || body[..start]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        let value_start = start + prefix.len();
        let length = body[value_start..].find('"')?;
        if at_boundary {
            return Some(&body[value_start..value_start + length]);
        }
        cursor = value_start + length;
    }
    None
}

/// Rectangle of a `bounds="[x1,y1][x2,y2]"` value.
fn bounds_rect(value: &str) -> Option<NodeBounds> {
    let numbers: Vec<u32> = value
        .split(['[', ']', ','])
        .filter_map(|part| part.parse().ok())
        .collect();
    match numbers.as_slice() {
        [left, top, right, bottom, ..] if right > left && bottom > top => Some(NodeBounds {
            left: *left,
            top: *top,
            right: *right,
            bottom: *bottom,
        }),
        _ => None,
    }
}

pub(crate) const CHECKIN_ENTRY_LABEL: &str = "晚点名签到";

pub(crate) fn detect_checkin_mode(view: &PageView) -> CheckinMode {
    let xml = &view.scoped_text;
    if xml.contains("外勤签到") {
        return CheckinMode::FieldWork;
    }
    if [
        "点击签到",
        "无法签到",
        "未到签到时间",
        "定位中",
        "签到成功",
        "已签到",
    ]
    .iter()
    .any(|label| xml.contains(label))
    {
        return CheckinMode::Ordinary;
    }
    CheckinMode::Unsupported
}

pub(crate) fn detect_page_route(view: &PageView) -> PageRoute {
    let xml = &view.scoped_text;
    if xml.contains("一键登录") || xml.contains("其他账户登录") {
        return PageRoute::Login;
    }

    // Page-level navigation labels outrank status words that may also appear
    // on a home/service-card summary (for example, an already-completed card).
    if xml.contains("我的服务") {
        return PageRoute::Home;
    }
    if xml.contains("业务") {
        return PageRoute::Services;
    }

    // A status/action word alone is not sufficient page identity: the same
    // text can occur on summaries, overlays, and unrelated pages. The observed
    // check-in hierarchy exposes its page title, so require that context.
    //
    // Note the same label is also the home page's entry card. Requiring it
    // together with a status/action word is what keeps the entry card (which
    // carries no status word) from being read as the check-in page.
    let has_checkin_title = xml.contains(CHECKIN_ENTRY_LABEL);
    let ordinary_checkin_markers = [
        "无法签到",
        "未到签到时间",
        "定位中",
        "点击签到",
        "签到成功",
        "已签到",
    ];
    let has_ordinary_checkin_marker = ordinary_checkin_markers
        .iter()
        .any(|marker| xml.contains(marker));
    let field_work_context_markers = ["未签到", "当前位置", "当前地点", "签到时间"];
    let field_work_context_count = field_work_context_markers
        .iter()
        .filter(|marker| xml.contains(**marker))
        .count();
    if has_checkin_title
        && (has_ordinary_checkin_marker
            || (xml.contains("外勤签到") && field_work_context_count >= 1))
    {
        return PageRoute::Checkin;
    }
    PageRoute::Unknown
}

/// One hierarchy read, already classified.
///
/// The raw XML and its parsed `PageView` travel together so a caller can make a
/// navigation decision and then reuse the same round's evidence, instead of
/// re-parsing or, worse, mixing rounds.
#[derive(Debug, Clone)]
pub(crate) struct PageObservation {
    pub(crate) view: PageView,
    /// Raw hierarchy XML for this observation.
    ///
    /// `view.scoped_text` is a flattened label blob; node boundaries and
    /// attributes (such as the `date` icon's `content-desc`) are only present
    /// in the raw document. Success-date extraction needs them, so the XML is
    /// carried alongside the parsed view instead of being re-derived.
    pub(crate) xml: String,
    pub(crate) route: PageRoute,
    pub(crate) checkin_mode: CheckinMode,
    /// `dump_succeeded`, `dump_failed`, `dump_empty_or_truncated` or
    /// `route_markers_missing`.
    pub(crate) status: &'static str,
    /// Package that owns a visible window but is not the target app.
    pub(crate) foreign_package: Option<String>,
}

impl PageObservation {
    fn unreadable(status: &'static str, target_package: &str) -> Self {
        Self {
            view: PageView::parse("", target_package),
            xml: String::new(),
            route: PageRoute::RouteCandidateUnconfirmed,
            checkin_mode: CheckinMode::Unsupported,
            status,
            foreign_package: None,
        }
    }
}

/// Read the page hierarchy once and classify it, emitting a structured
/// `hierarchy_observation` event carrying the dump status, byte count, UTF-8
/// validity and marker hit summary for this round (FR-011).
///
/// A dump that fails or comes back empty/truncated is reported as
/// `route_candidate_unconfirmed` rather than being folded into a generic
/// failure or an empty string.
pub(crate) fn observe_page_route(
    device: &adb::AdbDevice,
    log: &RunLog,
    stage: &str,
    observation_id: usize,
    target_package: &str,
) -> Result<PageObservation> {
    let observation = device.dump_hierarchy_observation(stage)?;
    let focused_window = device.focused_window().ok();
    let focused_package = focused_window
        .as_deref()
        .and_then(adb::package_from_focused_window);
    log.event(
        "hierarchy_observation",
        serde_json::json!({
            "observation_id": observation_id,
            "stage": observation.stage,
            "status": observation.status,
            "bytes": observation.bytes,
            "utf8_valid": observation.utf8_valid,
            "elapsed_ms": observation.elapsed_ms,
            "exit_code": observation.exit_code,
            "stderr_summary": observation.stderr_summary,
            "marker_summary": observation.marker_summary,
            "packages": observation.packages,
            "root_package": observation.root_package,
            "node_count": observation.node_count,
            "focused_window": focused_window,
            "focused_package": focused_package,
        }),
    )?;
    if observation.status != adb::HierarchyStatus::DumpSucceeded {
        let reason = observation.status.as_str();
        log.event(
            "hierarchy_unavailable",
            serde_json::json!({
                "observation_id": observation_id,
                "stage": stage,
                "status": observation.status,
                "reason": reason,
                "bytes": observation.bytes,
                "exit_code": observation.exit_code,
            }),
        )?;
        return Ok(PageObservation::unreadable(reason, target_package));
    }

    let xml = observation.xml.unwrap_or_default();
    let view = PageView::parse(&xml, target_package);
    let foreign_package = view
        .root_foreign_package()
        .filter(|package| {
            focused_package
                .as_deref()
                .is_none_or(|focused| focused == *package)
        })
        .map(str::to_string);
    if let Some(package) = &foreign_package {
        // The root package identifies the window owner; nested system-owned
        // nodes alone are not evidence that another window obscures the
        // target. Preserve the focused window in the hierarchy event too.
        log.event(
            "foreign_window",
            serde_json::json!({
                "observation_id": observation_id,
                "stage": stage,
                "package": package,
                "packages": view.packages,
                "root_package": observation.root_package,
                "node_count": view.node_count(),
                "focused_window": focused_window,
                "focused_package": focused_package,
            }),
        )?;
    }

    let route = detect_page_route(&view);
    // Valid XML with no usable page identity is its own state: the page could
    // not be identified, so it stays an unconfirmed candidate and is reported
    // as `route_markers_missing` rather than as a generic unknown page.
    if route == PageRoute::Unknown {
        log.event(
            "route_markers_missing",
            serde_json::json!({
                "observation_id": observation_id,
                "stage": stage,
                "marker_summary": observation.marker_summary,
                "packages": view.packages,
                "root_package": observation.root_package,
                "node_count": view.node_count(),
            }),
        )?;
        return Ok(PageObservation {
            view,
            xml,
            route: PageRoute::RouteCandidateUnconfirmed,
            checkin_mode: CheckinMode::Unsupported,
            status: "route_markers_missing",
            foreign_package,
        });
    }
    let checkin_mode = if route == PageRoute::Checkin {
        detect_checkin_mode(&view)
    } else {
        CheckinMode::Unsupported
    };
    Ok(PageObservation {
        view,
        xml,
        route,
        checkin_mode,
        status: adb::HierarchyStatus::DumpSucceeded.as_str(),
        foreign_package,
    })
}
