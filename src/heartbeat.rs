use serde::{Deserialize, Serialize};

use crate::command::{Command, CommandResult};

/// What the agent is doing, as of this beat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    /// No rental; ready for one.
    Idle,
    /// Pulling the runtime image and starting the container.
    Provisioning,
    /// A rental is live.
    Running,
    /// Finishing a rental, or refusing new work before an update.
    Draining,
    /// Something is wrong that the agent could not recover from.
    Error,
}

/// The agent's periodic check-in. The only thing that writes `last_seen_at`
/// server-side, and the channel commands are handed over on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatRequest {
    pub agent_version: String,
    pub uptime_seconds: u64,
    pub state: AgentState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rental_uuid: Option<String>,
    /// Claims about what this machine can do *right now*. A machine that stops
    /// advertising something a rental needs is unlisted rather than rented and
    /// then failing.
    pub capabilities: Vec<String>,
    pub telemetry: Telemetry,
    /// Results for commands handed over on an earlier beat. Acking on the next
    /// beat keeps the whole exchange to one round trip.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<CommandResult>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Telemetry {
    pub cpu_percent: f32,
    pub memory_used_mb: u64,
    pub disk_free_gb: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gpu: Vec<GpuTelemetry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuTelemetry {
    pub index: u8,
    pub utilization_percent: u8,
    /// How busy the video encoder is — the resource a rental actually
    /// competes with the renter's own broadcast for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoder_percent: Option<u8>,
    pub memory_used_mb: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature_c: Option<i16>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HeartbeatResponse {
    #[serde(default)]
    pub heartbeat_interval_seconds: Option<u64>,
    /// `suspended` or `retired` here means stop everything and idle.
    pub machine_status: String,
    #[serde(default)]
    pub rotate_credential: bool,
    #[serde(default)]
    pub minimum_agent_version: Option<String>,
    #[serde(default)]
    pub commands: Vec<Command>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RotateResponse {
    pub credential: String,
    /// How long the outgoing credential keeps working. The overlap is what
    /// stops a crash mid-rotation from locking the machine out of its own
    /// control plane.
    #[serde(default)]
    pub previous_valid_until: Option<String>,
}
