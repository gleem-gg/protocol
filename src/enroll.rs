use serde::{Deserialize, Serialize};

use crate::specs::SpecsReport;

/// Trades a one-time claim code for a long-lived machine credential.
///
/// The code is typed in by the host — from the dashboard into a terminal, or
/// at the appliance's first-boot screen — and is burned on first use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollRequest {
    pub code: String,
    pub agent_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hardware_fingerprint: Option<String>,
    pub specs: SpecsReport,
}

/// What the control plane hands back. `credential` is returned exactly once
/// and is not recoverable afterwards — only its hash is stored server-side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollResponse {
    pub machine_uuid: String,
    pub credential: String,
    pub heartbeat_interval_seconds: u64,
    #[serde(default)]
    pub gateway_url: Option<String>,
}
