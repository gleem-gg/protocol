//! Frames exchanged over the signalling channel.
//!
//! The gateway relays these between a browser and an agent without
//! understanding them — it never parses SDP. It is a broker, not a
//! participant, which is what keeps it out of the media path and out of the
//! business of knowing anything about codecs.

use serde::{Deserialize, Serialize};

/// One signalling message.
///
/// `session` identifies which rental session a frame belongs to. A machine
/// holds one connection and, today, one rental — but tagging every frame means
/// the channel does not have to be torn down and rebuilt if that ever changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    pub session: String,
    #[serde(flatten)]
    pub body: FrameBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FrameBody {
    /// A browser has attached. Tells the far end to start offering.
    Open,

    /// One signalling message, relayed verbatim.
    ///
    /// The payload is an opaque string and nothing between the two peers
    /// looks inside it. That is not squeamishness about SDP: the desktop
    /// server in the container speaks its own protocol, and a gateway that
    /// understood it would have to be redeployed every time that protocol
    /// changed. Keeping it opaque means the container can be swapped for a
    /// different implementation without touching the gateway at all.
    Signal { payload: String },

    /// Periodic connection quality from the browser. Terminates at the
    /// gateway, which forwards it to the control plane rather than the
    /// machine — a host has no business seeing a renter's session telemetry
    /// in real time.
    Stats {
        #[serde(default)]
        bitrate_kbps: Option<u32>,
        #[serde(default)]
        framerate: Option<u32>,
        #[serde(default)]
        rtt_ms: Option<u32>,
        #[serde(default)]
        packet_loss: Option<f32>,
    },

    /// Stop now. Sent to both ends when a rental ends or a session is killed.
    Terminate {
        #[serde(default)]
        reason: Option<String>,
    },

    /// Something went wrong that the other end should hear about.
    Error { message: String },

    /// A renter's tool connected to the rental's obs-websocket through the
    /// gateway. `session` is the connection's id, one per client socket, so a
    /// Stream Deck and a chat bot can be connected at the same time without
    /// touching the desktop stream.
    ObsOpen,

    /// One obs-websocket message, relayed verbatim in either direction. JSON
    /// text only: the gateway offers the `obswebsocket.json` subprotocol and
    /// nothing else, since binary does not cross this channel.
    ObsMessage { payload: String },

    /// One side of an obs-websocket connection went away. Ends that
    /// connection only, never the machine's channel, unlike `Terminate`.
    ObsClose {
        #[serde(default)]
        reason: Option<String>,
    },
}

impl Frame {
    pub fn new(session: impl Into<String>, body: FrameBody) -> Self {
        Self { session: session.into(), body }
    }

    pub fn terminate(session: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::new(session, FrameBody::Terminate { reason: Some(reason.into()) })
    }

    /// Wrap one opaque signalling message for the far end.
    pub fn signal(session: impl Into<String>, payload: impl Into<String>) -> Self {
        Self::new(session, FrameBody::Signal { payload: payload.into() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialises_with_a_flat_type_tag() {
        let frame = Frame::new("s-1", FrameBody::Open);
        let json = serde_json::to_value(&frame).unwrap();

        assert_eq!(json["session"], "s-1");
        assert_eq!(json["type"], "open");
    }

    #[test]
    fn relays_a_payload_without_looking_inside_it() {
        // Whatever the desktop server speaks travels through untouched, so
        // the gateway needs no opinion about it.
        let opaque = r#"{"sdp":{"type":"offer","sdp":"v=0..."},"selkies":"whatever"}"#;
        let frame = Frame::signal("s-1", opaque);

        let decoded: Frame = serde_json::from_str(&serde_json::to_string(&frame).unwrap()).unwrap();

        match decoded.body {
            FrameBody::Signal { payload } => assert_eq!(payload, opaque),
            other => panic!("expected a signal, got {other:?}"),
        }
    }

    #[test]
    fn rejects_a_frame_with_no_session() {
        // Untagged frames would be relayed to whoever happened to be attached.
        let result: Result<Frame, _> = serde_json::from_str(r#"{"type":"open"}"#);

        assert!(result.is_err());
    }

    #[test]
    fn round_trips_stats() {
        let frame = Frame::new(
            "s-1",
            FrameBody::Stats {
                bitrate_kbps: Some(7800),
                framerate: Some(30),
                rtt_ms: Some(24),
                packet_loss: Some(0.1),
            },
        );

        let decoded: Frame = serde_json::from_str(&serde_json::to_string(&frame).unwrap()).unwrap();

        match decoded.body {
            FrameBody::Stats { bitrate_kbps, .. } => assert_eq!(bitrate_kbps, Some(7800)),
            other => panic!("expected stats, got {other:?}"),
        }
    }
}
