//! The rental file explorer: browsing a running rental's `/workspace` and
//! moving files out of it.
//!
//! Two paths, split by size. Small JSON requests (list a folder, start a
//! download) ride the machine's existing signalling channel as
//! [`FilesRequest`] and [`FilesResponse`] frames, exactly like the
//! obs-websocket relay. File contents never do: that channel carries text and
//! the desktop's own signalling, and a large download on it would starve the
//! stream. Each transfer gets a WebSocket of its own from the agent to the
//! gateway instead, speaking [`TransferMessage`] text frames around binary
//! chunks, and the gateway pipes it into a plain HTTP download for the
//! browser. Nothing is stored on the way.
//!
//! The browser speaks the inner request and response objects directly, as
//! JSON on its files socket; only the machine leg carries the frame envelope.

use serde::{Deserialize, Serialize};

/// Bytes per binary frame on a transfer socket.
pub const CHUNK_BYTES: usize = 256 * 1024;

/// The largest single file a renter may download. A zip of a folder is held
/// to the same limit per file inside it, not in total.
pub const MAX_DOWNLOAD_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// Transfers one rental may run at once. Every byte crosses the gateway's
/// host twice, and that host also serves the website.
pub const MAX_TRANSFERS_PER_RENTAL: usize = 3;

/// A transfer that moves nothing for this long is dropped on both ends.
pub const STALL_SECONDS: u64 = 60;

/// How long a transfer id stays claimable after the gateway minted it.
pub const TRANSFER_TTL_SECONDS: u64 = 60;

/// The longest path a request may name, in bytes.
pub const MAX_PATH_BYTES: usize = 1024;

/// What the browser asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilesOp {
    /// The entries of one folder.
    List,
    /// A file as itself, or a folder as one zip.
    Download,
    /// An operation this build does not know. Answered with
    /// [`FilesError::Unsupported`] rather than dropped, so a newer browser
    /// talking to an older agent gets an answer instead of a hang.
    #[serde(other)]
    Unsupported,
}

/// One request, from the browser through the gateway to the agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilesRequest {
    /// Chosen by the browser to match the response; opaque to everything else.
    pub id: String,
    pub op: FilesOp,
    /// Relative to the workspace, `/`-separated, `""` for the root. See
    /// [`is_safe_path`].
    #[serde(default)]
    pub path: String,
    /// Set by the gateway on a download: the id the browser fetches the file
    /// under, and the agent dials in on. Whatever the browser put here is
    /// thrown away.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transfer: Option<String>,
    /// Set by the gateway on a download: what the agent presents when it
    /// dials the transfer socket. Never sent to the browser.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

/// The answer to one request, from the agent through the gateway to the
/// browser.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilesResponse {
    pub id: String,
    pub ok: bool,
    /// For `list`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entries: Option<Vec<FileEntry>>,
    /// For `list`: the folder had more entries than were listed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    /// For `download`: the id to fetch from the gateway.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transfer: Option<String>,
    /// For `download`: the file name the browser saves under.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// For `download`: the size in bytes, when known up front. A zip is built
    /// as it streams, so its size is not.
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<FilesError>,
}

impl FilesResponse {
    pub fn failed(id: impl Into<String>, error: FilesError) -> Self {
        Self { id: id.into(), ok: false, error: Some(error), ..Self::default() }
    }
}

/// Why a request failed. Codes, not prose: the browser translates them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilesError {
    NotFound,
    /// A symlink, or a path the kernel refused to resolve beneath the
    /// workspace.
    NotAllowed,
    InvalidPath,
    /// A file over [`MAX_DOWNLOAD_BYTES`].
    TooLarge,
    TooManyTransfers,
    Unsupported,
    MachineUnavailable,
    /// Anything else, including codes a newer build sends that this one does
    /// not know.
    #[serde(other)]
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub kind: EntryKind,
    /// Bytes, for files; 0 otherwise.
    pub size: u64,
    /// Unix seconds.
    pub modified: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File,
    Dir,
    /// A symlink. Listed so the renter can see it is there, but never opened,
    /// renamed onto or deleted through: one could point at the machine's
    /// credential.
    Link,
}

/// The text frames on a transfer socket, around the binary chunks: a header
/// first, then exactly one of done or failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TransferMessage {
    Header {
        name: String,
        #[serde(default)]
        size: Option<u64>,
    },
    Done {
        done: bool,
        bytes: u64,
    },
    Failed {
        error: String,
    },
}

/// Whether a request path stays inside the workspace on its face: relative,
/// no `.` or `..` components, no empty components, no NUL, bounded length.
///
/// Checked by the gateway before relaying, so a bad request never reaches the
/// host. The agent still opens everything with `openat2` beneath the
/// workspace, which is what actually holds; this is the cheap first line.
pub fn is_safe_path(path: &str) -> bool {
    if path.is_empty() {
        return true;
    }

    path.len() <= MAX_PATH_BYTES
        && !path.contains('\0')
        && path.split('/').all(|component| !matches!(component, "" | "." | ".."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signalling::{Frame, FrameBody};

    #[test]
    fn accepts_plain_relative_paths() {
        for path in ["", "recordings", "media/fonts/Brand.ttf", "recordings/2026-11-02 20-15-03.mkv", "a..b/.hidden"] {
            assert!(is_safe_path(path), "{path:?} should be allowed");
        }
    }

    #[test]
    fn refuses_paths_that_could_leave_the_workspace() {
        for path in ["/etc/passwd", "..", "../credential", "media/../../x", "media/./x", "media//x", "media/", "a\0b"] {
            assert!(!is_safe_path(path), "{path:?} should be refused");
        }

        assert!(!is_safe_path(&"a/".repeat(600)));
    }

    #[test]
    fn a_request_travels_inside_a_flat_frame() {
        let frame = Frame::new(
            "files-1",
            FrameBody::FilesRequest(FilesRequest {
                id: "7".into(),
                op: FilesOp::Download,
                path: "recordings".into(),
                transfer: Some("t".into()),
                token: Some("k".into()),
            }),
        );

        let json = serde_json::to_value(&frame).unwrap();
        assert_eq!(json["type"], "files_request");
        assert_eq!(json["session"], "files-1");
        assert_eq!(json["op"], "download");
        assert_eq!(json["token"], "k");

        let decoded: Frame = serde_json::from_value(json).unwrap();
        assert!(matches!(decoded.body, FrameBody::FilesRequest(FilesRequest { op: FilesOp::Download, .. })));
    }

    #[test]
    fn the_browser_shape_parses_without_the_gateway_fields() {
        let request: FilesRequest = serde_json::from_str(r#"{"id":"1","op":"list","path":"media"}"#).unwrap();
        assert_eq!(request.op, FilesOp::List);
        assert_eq!(request.transfer, None);

        let root: FilesRequest = serde_json::from_str(r#"{"id":"1","op":"list"}"#).unwrap();
        assert_eq!(root.path, "");
    }

    #[test]
    fn an_unknown_op_is_kept_and_answerable() {
        let request: FilesRequest = serde_json::from_str(r#"{"id":"1","op":"rename","path":"a"}"#).unwrap();
        assert_eq!(request.op, FilesOp::Unsupported);
    }

    #[test]
    fn an_unknown_error_code_reads_as_failed() {
        let response: FilesResponse = serde_json::from_str(r#"{"id":"1","ok":false,"error":"quota_exceeded"}"#).unwrap();
        assert_eq!(response.error, Some(FilesError::Failed));
    }

    #[test]
    fn a_response_serialises_only_what_it_carries() {
        let json = serde_json::to_value(FilesResponse::failed("3", FilesError::TooLarge)).unwrap();
        assert_eq!(json, serde_json::json!({"id": "3", "ok": false, "size": null, "error": "too_large"}));
    }

    #[test]
    fn transfer_messages_tell_themselves_apart() {
        let header: TransferMessage = serde_json::from_str(r#"{"name":"recordings.zip","size":null}"#).unwrap();
        assert_eq!(header, TransferMessage::Header { name: "recordings.zip".into(), size: None });

        let done: TransferMessage = serde_json::from_str(r#"{"done":true,"bytes":12}"#).unwrap();
        assert_eq!(done, TransferMessage::Done { done: true, bytes: 12 });

        let failed: TransferMessage = serde_json::from_str(r#"{"error":"the file changed"}"#).unwrap();
        assert_eq!(failed, TransferMessage::Failed { error: "the file changed".into() });
    }
}
