use serde::{Deserialize, Serialize};

/// A full hardware report. Sent at enrolment, on every boot, and whenever the
/// detected hardware changes — deliberately not on the heartbeat, so the
/// 20-second beat stays small.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpecsReport {
    pub cpu: CpuSpecs,
    pub memory_mb: u64,
    pub disk_gb: u64,
    pub gpus: Vec<GpuSpecs>,
    pub network: NetworkSpecs,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hardware_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CpuSpecs {
    pub model: String,
    pub cores: u16,
    pub threads: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuSpecs {
    /// Slot ordinal. The control plane syncs GPU rows against this, so a card
    /// being pulled removes its row rather than leaving a phantom behind.
    pub index: u8,
    pub vendor: String,
    pub model: String,
    /// Drives which codecs may be offered — Ampere has no AV1 encoder, Ada
    /// does. Stored so the marketplace never has to guess from a model name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub architecture: Option<String>,
    pub vram_mb: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nvenc_sessions: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub driver_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pci_bus_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_uuid: Option<String>,
}

/// Where the machine is and what its connection can do.
///
/// The bandwidth fields are `None` until something has actually measured them.
/// The NIC's link rate is not the machine's internet throughput, and reporting
/// one as the other would put a number in the marketplace that renters would
/// reasonably rely on and that nobody had verified.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkSpecs {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upload_mbps: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_mbps: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
}
