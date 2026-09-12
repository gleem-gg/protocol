use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Work the control plane asks the agent to do.
///
/// Unknown is not a failure mode to paper over: a control plane newer than the
/// agent will send kinds this build has never heard of, and the agent has to
/// report that honestly rather than silently drop them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandKind {
    StartRental,
    StopRental,
    WipeWorkspace,
    PullRuntimeImage,
    UpdateAgent,
    Reboot,
    #[serde(other)]
    Unknown,
}

impl CommandKind {
    /// Whether the command destroys data or interrupts service. These carry a
    /// `confirm` flag, and the agent re-checks its own preconditions before
    /// acting rather than trusting the control plane's view of the machine.
    pub fn is_destructive(&self) -> bool {
        matches!(self, Self::WipeWorkspace | Self::Reboot)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Command {
    pub uuid: String,
    #[serde(rename = "type")]
    pub kind: CommandKind,
    /// After this the command must not run. A stop that arrives an hour late
    /// is worse than one that never arrived at all.
    pub expires_at: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandStatus {
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub uuid: String,
    pub status: CommandStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl CommandResult {
    pub fn succeeded(uuid: impl Into<String>, result: Option<Value>) -> Self {
        Self { uuid: uuid.into(), status: CommandStatus::Succeeded, result, error: None }
    }

    pub fn failed(uuid: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            uuid: uuid.into(),
            status: CommandStatus::Failed,
            result: None,
            error: Some(error.into()),
        }
    }
}
