use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    pub(crate) schema_version: u32,
    pub(crate) key: String,
    pub(crate) release_id: String,
    pub(crate) release_root: String,
    pub(crate) authority: Authority,
    pub(crate) overlap: OverlapPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) timeout_seconds: Option<u64>,
    #[serde(default)]
    pub(crate) arguments: Vec<String>,
    pub(crate) cwd: String,
    pub(crate) schedule: Schedule,
    pub(crate) launch: LaunchImage,
    #[serde(default)]
    pub(crate) environment: BTreeMap<String, String>,
    pub(crate) output: Output,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Authority {
    CurrentUserBackground,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum OverlapPolicy {
    Skip,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum Schedule {
    Interval {
        seconds: u64,
        #[serde(default)]
        run_at_load: bool,
    },
    LocalCalendar {
        hour: u8,
        minute: u8,
        #[serde(default)]
        run_at_load: bool,
    },
}

impl Schedule {
    pub(crate) fn run_at_load(&self) -> bool {
        match self {
            Self::Interval { run_at_load, .. } | Self::LocalCalendar { run_at_load, .. } => {
                *run_at_load
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum LaunchImage {
    Direct {
        program: String,
        sha256: String,
    },
    Interpreted {
        interpreter: String,
        interpreter_sha256: String,
        script: String,
        script_sha256: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Output {
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DefinitionRecord {
    pub(crate) digest: String,
    pub(crate) key: String,
    pub(crate) registered_at: i64,
    pub(crate) manifest: Manifest,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DefinitionSummary {
    pub(crate) digest: String,
    pub(crate) key: String,
    pub(crate) registered_at: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct BindingRecord {
    pub(crate) key: String,
    pub(crate) definition_digest: Option<String>,
    pub(crate) enabled: bool,
    #[serde(skip_serializing)]
    pub(crate) plist_sha256: Option<String>,
    pub(crate) updated_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ActivationState {
    StartFailed,
    Running,
    Exited,
    Signaled,
    TimedOut,
    SkippedOverlap,
    Lost,
}

impl ActivationState {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::StartFailed => "start_failed",
            Self::Running => "running",
            Self::Exited => "exited",
            Self::Signaled => "signaled",
            Self::TimedOut => "timed_out",
            Self::SkippedOverlap => "skipped_overlap",
            Self::Lost => "lost",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "start_failed" => Some(Self::StartFailed),
            "running" => Some(Self::Running),
            "exited" => Some(Self::Exited),
            "signaled" => Some(Self::Signaled),
            "timed_out" => Some(Self::TimedOut),
            "skipped_overlap" => Some(Self::SkippedOverlap),
            "lost" => Some(Self::Lost),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Trigger {
    Manual,
    Launchd,
}

impl Trigger {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Launchd => "launchd",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ActivationRecord {
    pub(crate) id: String,
    pub(crate) key: String,
    pub(crate) definition_digest: String,
    pub(crate) trigger: Trigger,
    pub(crate) state: ActivationState,
    pub(crate) admitted_at: i64,
    pub(crate) started_at: Option<i64>,
    pub(crate) finished_at: Option<i64>,
    pub(crate) broker_pid: Option<u32>,
    pub(crate) child_pid: Option<u32>,
    pub(crate) exit_code: Option<i32>,
    pub(crate) signal: Option<i32>,
    pub(crate) detail: Option<String>,
}
