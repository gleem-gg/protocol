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
    /// The runtime image rentals on this machine currently start from. The
    /// agent pulls it in the background while it has no rental, so the first
    /// rental after enrolment, or after a new image is published, does not
    /// spend its first minutes downloading. Absent from control planes that
    /// predate it.
    #[serde(default)]
    pub runtime_image: Option<RuntimeImage>,
}

/// A runtime image as the control plane names it in a rental spec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeImage {
    pub image: String,
    /// Pinned in production, so a moved tag cannot change what runs on
    /// somebody else's hardware. Absent only for a developer's local image.
    #[serde(default)]
    pub digest: Option<String>,
}

impl RuntimeImage {
    /// The exact reference to pull: by digest when there is one.
    pub fn reference(&self) -> String {
        match self.digest.as_deref() {
            Some(digest) if !digest.is_empty() => format!("{}@{}", self.image, digest),
            _ => self.image.clone(),
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn response(extra: serde_json::Value) -> HeartbeatResponse {
        let mut body = serde_json::json!({"machine_status": "active"});
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        serde_json::from_value(body).unwrap()
    }

    #[test]
    fn reads_a_response_from_a_control_plane_that_names_no_runtime_image() {
        assert_eq!(response(serde_json::json!({})).runtime_image, None);
    }

    #[test]
    fn pins_the_runtime_image_by_digest() {
        let image = response(serde_json::json!({"runtime_image": {
            "image": "ghcr.io/gleem-gg/obs-runtime",
            "digest": "sha256:abc"
        }}))
        .runtime_image
        .unwrap();

        assert_eq!(image.reference(), "ghcr.io/gleem-gg/obs-runtime@sha256:abc");
    }

    #[test]
    fn falls_back_to_the_tag_when_no_digest_is_configured() {
        // gleem.gg sends `null` when HOSTING_RUNTIME_DIGEST is unset.
        let image = response(serde_json::json!({"runtime_image": {
            "image": "ghcr.io/gleem-gg/obs-runtime",
            "digest": null
        }}))
        .runtime_image
        .unwrap();

        assert_eq!(image.reference(), "ghcr.io/gleem-gg/obs-runtime");
    }
}
