//! Wire types shared between the Gleem machine agent and the control plane,
//! and (from M3) the signalling gateway.
//!
//! Every type here has a counterpart in the Laravel application, so the field
//! names are chosen to match its JSON exactly rather than to read naturally in
//! Rust. Keeping them in one crate is the point: a change to the wire format
//! breaks the build on both sides instead of at runtime on somebody's machine.

pub mod command;
pub mod enroll;
pub mod heartbeat;
pub mod signalling;
pub mod specs;
pub mod ticket;

pub use command::{Command, CommandKind, CommandResult, CommandStatus};
pub use enroll::{EnrollRequest, EnrollResponse};
pub use heartbeat::{
    AgentState, GpuTelemetry, HeartbeatRequest, HeartbeatResponse, RotateResponse, Telemetry,
};
pub use signalling::{Frame, FrameBody};
pub use specs::{CpuSpecs, GpuSpecs, NetworkSpecs, SpecsReport};
pub use ticket::{TicketError, TicketPayload};

/// Capability strings an agent advertises. The control plane refuses a rental
/// unless the machine is currently reporting everything a session needs, so
/// these are claims about right now — never about what was once true.
pub mod capability {
    /// At least one NVIDIA GPU is visible to the agent.
    pub const GPU_NVIDIA: &str = "gpu.nvidia";
    /// The GPU can encode H.264 in hardware. The only codec Gleem negotiates.
    pub const NVENC_H264: &str = "nvenc.h264";
    /// The GPU can encode HEVC. Advertised but not yet negotiated.
    pub const NVENC_HEVC: &str = "nvenc.hevc";
    /// The GPU can encode AV1 — Ada and newer only.
    pub const NVENC_AV1: &str = "nvenc.av1";
    /// A working container runtime is installed.
    pub const CONTAINER_PODMAN: &str = "container.podman";
    /// Per-rental encrypted workspaces can be created.
    pub const WORKSPACE_LUKS: &str = "workspace.luks";
    /// The signalling gateway answered a health check.
    pub const GATEWAY_REACHABLE: &str = "gateway.reachable";
    /// The TURN relay answered a STUN binding request.
    pub const TURN_REACHABLE: &str = "turn.reachable";
}
