//! Typed command requests shared by the CLI adapters, the GUI, and the run
//! state machine.
//!
//! Nothing in this module depends on `clap`: the CLI layer parses its own
//! argument structs and converts them here, so GUI code never has to construct
//! (or compile against) a command-line parser type.

use crate::domain::Mode;

/// One instance selector, independent of how it was parsed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstanceTargetArgs {
    pub instance_index: Option<u32>,
    pub serial: Option<String>,
}

/// The mode the caller asked for, before configuration defaults are applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestedMode {
    DryRun,
    Live,
    FromConfig,
}

/// A request to run the check-in state machine.
#[derive(Debug, Clone)]
pub struct RunRequest {
    pub requested_mode: RequestedMode,
    pub explicit_confirmation: bool,
    pub all_instances: bool,
    pub json: bool,
    pub target: InstanceTargetArgs,
}

/// Where a run request came from. Recorded for diagnostics and future policy
/// differences (for example, a GUI confirmation dialog vs a CLI flag).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunSource {
    Cli,
    Gui,
}

impl RunSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Gui => "gui",
        }
    }
}

/// A resolved run request: configuration defaults have been applied.
#[derive(Debug, Clone)]
pub struct RunPolicy {
    pub mode: Mode,
    pub explicit_confirmation: bool,
    pub all_instances: bool,
    pub json: bool,
    pub target: InstanceTargetArgs,
    pub source: RunSource,
}

impl RunRequest {
    /// Resolve the requested mode against `runtime.mode` when the caller did
    /// not pass an explicit `--dry-run` / `--live`.
    pub fn resolve(self, configured_mode: Mode, source: RunSource) -> RunPolicy {
        let mode = match self.requested_mode {
            RequestedMode::DryRun => Mode::DryRun,
            RequestedMode::Live => Mode::Live,
            RequestedMode::FromConfig => configured_mode,
        };
        RunPolicy {
            mode,
            explicit_confirmation: self.explicit_confirmation,
            all_instances: self.all_instances,
            json: self.json,
            target: self.target,
            source,
        }
    }
}

/// A Windows scheduled-task request.
#[derive(Debug, Clone)]
pub enum TaskRequest {
    Create {
        name: String,
        live: bool,
        confirm: bool,
        dry_run: bool,
        all_instances: bool,
        target: InstanceTargetArgs,
    },
    Delete {
        name: String,
        confirm: bool,
    },
    Query {
        name: String,
    },
}

/// A configuration query/action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigRequest {
    Path,
    Show,
    Open,
}
