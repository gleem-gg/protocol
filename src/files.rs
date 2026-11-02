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
//! Uploads run the same socket the other way: the browser POSTs the file to
//! the gateway, which streams it to the agent, which writes it to a hidden
//! part file and renames it into place only once every byte has arrived.
//!
//! The browser speaks the inner request and response objects directly, as
//! JSON on its files socket; only the machine leg carries the frame envelope.

use serde::{Deserialize, Serialize};

/// Bytes per binary frame on a transfer socket.
pub const CHUNK_BYTES: usize = 256 * 1024;

/// The largest single file a renter may upload.
pub const MAX_UPLOAD_BYTES: u64 = 1024 * 1024 * 1024;

/// Space an upload must leave free on the machine's disk, so a renter cannot
/// fill the host's disk to the last byte.
pub const UPLOAD_HEADROOM_BYTES: u64 = 1024 * 1024 * 1024;

/// The suffix of a file still being uploaded. The name also starts with a
/// dot, so the panel hides it, and the setup save leaves it out.
pub const PART_SUFFIX: &str = ".gleem-part";

/// The workspace's own folders, which can be neither deleted nor renamed.
pub const PROTECTED_FOLDERS: [&str; 3] = ["config", "media", "recordings"];

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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilesOp {
    #[default]
    /// The entries of one folder.
    List,
    /// A file as itself, or a folder as one zip.
    Download,
    /// A file into the workspace, `size` bytes, creating missing folders on
    /// the way. Lands under its name only once every byte has arrived.
    Upload,
    /// One new folder; its parent must exist.
    Mkdir,
    /// Move or rename `path` to `to`. Never replaces anything.
    Rename,
    /// A file, a symlink itself, or a folder with everything in it.
    Delete,
    /// An operation this build does not know. Answered with
    /// [`FilesError::Unsupported`] rather than dropped, so a newer browser
    /// talking to an older agent gets an answer instead of a hang.
    #[serde(other)]
    Unsupported,
}

/// One request, from the browser through the gateway to the agent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
    /// For `rename`: where to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// For `upload`: the file's size in bytes, as the browser read it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// For `upload`: replace a file already under that name.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub replace: bool,
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
    /// A download over [`MAX_DOWNLOAD_BYTES`], or an upload over
    /// [`MAX_UPLOAD_BYTES`].
    TooLarge,
    TooManyTransfers,
    /// Something is already there: an upload without `replace`, a new
    /// folder, or a rename target.
    Exists,
    /// The machine's disk would be left with less than
    /// [`UPLOAD_HEADROOM_BYTES`] free.
    InsufficientSpace,
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

/// The text frames on a transfer socket, around the binary chunks.
///
/// A download: the agent sends a header, the chunks, then done or failed.
/// An upload: the gateway sends the chunks then done with the byte count, and
/// the agent answers done once the file is in place under its name, or
/// failed.
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
/// Whether a path names one of [`PROTECTED_FOLDERS`] or the root itself,
/// which the panel may list but never delete, rename or replace.
pub fn is_protected(path: &str) -> bool {
    path.is_empty() || PROTECTED_FOLDERS.contains(&path)
}

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
                ..FilesRequest::default()
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
        let request: FilesRequest = serde_json::from_str(r#"{"id":"1","op":"chmod","path":"a"}"#).unwrap();
        assert_eq!(request.op, FilesOp::Unsupported);
    }

    #[test]
    fn reads_the_managing_requests() {
        let upload: FilesRequest =
            serde_json::from_str(r#"{"id":"1","op":"upload","path":"media/a.png","size":12,"replace":true}"#).unwrap();
        assert_eq!((upload.op, upload.size, upload.replace), (FilesOp::Upload, Some(12), true));

        let rename: FilesRequest =
            serde_json::from_str(r#"{"id":"2","op":"rename","path":"media/a.png","to":"media/b.png"}"#).unwrap();
        assert_eq!((rename.op, rename.to.as_deref()), (FilesOp::Rename, Some("media/b.png")));

        let plain: FilesRequest = serde_json::from_str(r#"{"id":"3","op":"mkdir","path":"media/x"}"#).unwrap();
        assert!(!plain.replace);
        assert!(!serde_json::to_string(&plain).unwrap().contains("replace"));
    }

    #[test]
    fn protects_the_workspace_folders_but_not_what_is_in_them() {
        assert!(is_protected(""));
        assert!(is_protected("media"));
        assert!(is_protected("config"));
        assert!(!is_protected("media/intro.mp4"));
        assert!(!is_protected("mediathek"));
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
