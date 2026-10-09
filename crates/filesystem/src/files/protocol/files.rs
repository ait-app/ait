//! Paseo workspace filesystem request and response payloads.

use serde::{Deserialize, Serialize};

/// Entry Kind discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EntryKind {
    /// File value.
    File,
    /// Directory value.
    Directory,
}

/// Explorer Mode discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExplorerMode {
    /// List value.
    List,
    /// File value.
    File,
}

/// Match Mode discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MatchMode {
    /// Fuzzy value.
    Fuzzy,
    /// Suffix value.
    Suffix,
}

/// File Kind discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FileKind {
    /// Text value.
    Text,
    /// Image value.
    Image,
    /// Binary value.
    Binary,
}

/// Encoding discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Encoding {
    /// Utf8 value.
    #[serde(rename = "utf-8")]
    Utf8,
    /// Base64 value.
    Base64,
    /// None value.
    None,
}

/// Suggestions Request payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SuggestionsRequest {
    /// query.
    pub(crate) query: String,
    /// cwd.
    pub(crate) cwd: Option<String>,
    /// include files.
    pub(crate) include_files: Option<bool>,
    /// include directories.
    pub(crate) include_directories: Option<bool>,
    /// match mode.
    pub(crate) match_mode: Option<MatchMode>,
    /// limit.
    pub(crate) limit: Option<usize>,
}

/// Explorer Request payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExplorerRequest {
    /// cwd.
    pub(crate) cwd: String,
    /// path.
    pub(crate) path: Option<String>,
    /// mode.
    pub(crate) mode: ExplorerMode,
    /// accept binary.
    pub(crate) accept_binary: Option<bool>,
    /// max bytes.
    pub(crate) max_bytes: Option<u64>,
}

/// File Path Request payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FilePathRequest {
    /// cwd.
    pub(crate) cwd: String,
    /// path.
    pub(crate) path: String,
}

/// Subscribe Request payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SubscribeRequest {
    /// cwd.
    pub(crate) cwd: String,
    /// path.
    pub(crate) path: String,
    /// subscription id.
    pub(crate) subscription_id: Option<String>,
}

/// Unsubscribe Request payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UnsubscribeRequest {
    /// subscription id.
    pub(crate) subscription_id: String,
}

/// Write Request payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WriteRequest {
    /// cwd.
    pub(crate) cwd: String,
    /// path.
    pub(crate) path: String,
    /// content.
    pub(crate) content: String,
    /// expected modified at.
    pub(crate) expected_modified_at: String,
    /// expected revision.
    pub(crate) expected_revision: Option<String>,
}

/// Create Request payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateRequest {
    /// cwd.
    pub(crate) cwd: String,
    /// parent path.
    pub(crate) parent_path: String,
    /// name.
    pub(crate) name: String,
    /// kind.
    pub(crate) kind: EntryKind,
}

/// Rename Request payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RenameRequest {
    /// cwd.
    pub(crate) cwd: String,
    /// path.
    pub(crate) path: String,
    /// name.
    pub(crate) name: String,
}

/// Upload Request payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UploadRequest {
    /// file name.
    pub(crate) file_name: String,
    /// mime type.
    pub(crate) mime_type: String,
    /// size.
    pub(crate) size: u64,
    /// modified at.
    modified_at: String,
}

/// File Entry payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileEntry {
    /// name.
    pub(crate) name: String,
    /// path.
    pub(crate) path: String,
    /// kind.
    pub(crate) kind: EntryKind,
    /// size.
    pub(crate) size: u64,
    /// modified at.
    pub(crate) modified_at: String,
}

/// Directory payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Directory {
    /// path.
    pub(crate) path: String,
    /// entries.
    pub(crate) entries: Vec<FileEntry>,
}

/// File Preview payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FilePreview {
    /// path.
    pub(crate) path: String,
    /// kind.
    pub(crate) kind: FileKind,
    /// encoding.
    pub(crate) encoding: Encoding,
    /// content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) content: Option<String>,
    /// mime type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) mime_type: Option<String>,
    /// size.
    pub(crate) size: u64,
    /// modified at.
    pub(crate) modified_at: String,
    /// revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) revision: Option<String>,
}

/// Explorer Result payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExplorerResult {
    /// cwd.
    pub(crate) cwd: String,
    /// path.
    pub(crate) path: String,
    /// mode.
    pub(crate) mode: ExplorerMode,
    /// directory.
    pub(crate) directory: Option<Directory>,
    /// file.
    pub(crate) file: Option<FilePreview>,
    /// error.
    pub(crate) error: Option<String>,
}

/// Suggestion payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Suggestion {
    /// path.
    pub(crate) path: String,
    /// kind.
    pub(crate) kind: EntryKind,
}

/// Suggestions Result payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SuggestionsResult {
    /// directories.
    pub(crate) directories: Vec<String>,
    /// entries.
    pub(crate) entries: Vec<Suggestion>,
    /// error.
    pub(crate) error: Option<String>,
}

/// Subscribe Result payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SubscribeResult {
    /// subscription id.
    pub(crate) subscription_id: String,
    /// initial.
    pub(crate) initial: FileVersion,
}

/// File Update payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileUpdate {
    /// subscription id.
    pub(crate) subscription_id: String,
    /// version.
    pub(crate) version: FileVersion,
}

/// Write Result payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WriteResult {
    /// result.
    pub(crate) result: WriteOutcome,
}

/// Create Result payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateResult {
    /// cwd.
    pub(crate) cwd: String,
    /// parent path.
    pub(crate) parent_path: String,
    /// path.
    pub(crate) path: Option<String>,
    /// success.
    pub(crate) success: bool,
    /// error.
    pub(crate) error: Option<String>,
}

/// Rename Result payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RenameResult {
    /// cwd.
    pub(crate) cwd: String,
    /// path.
    pub(crate) path: String,
    /// renamed path.
    pub(crate) renamed_path: Option<String>,
    /// success.
    pub(crate) success: bool,
    /// error.
    pub(crate) error: Option<String>,
}

/// Duplicate Result payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DuplicateResult {
    /// cwd.
    pub(crate) cwd: String,
    /// path.
    pub(crate) path: String,
    /// duplicated path.
    pub(crate) duplicated_path: Option<String>,
    /// success.
    pub(crate) success: bool,
    /// error.
    pub(crate) error: Option<String>,
}

/// Delete Result payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteResult {
    /// cwd.
    pub(crate) cwd: String,
    /// path.
    pub(crate) path: String,
    /// success.
    pub(crate) success: bool,
    /// error.
    pub(crate) error: Option<String>,
}

/// Download Result payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DownloadResult {
    /// cwd.
    pub(crate) cwd: String,
    /// path.
    pub(crate) path: String,
    /// token.
    pub(crate) token: Option<String>,
    /// file name.
    pub(crate) file_name: Option<String>,
    /// mime type.
    pub(crate) mime_type: Option<String>,
    /// size.
    pub(crate) size: Option<u64>,
    /// error.
    pub(crate) error: Option<String>,
}

/// Uploaded File payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UploadedFile {
    /// id.
    pub(crate) id: String,
    /// file name.
    pub(crate) file_name: String,
    /// mime type.
    pub(crate) mime_type: String,
    /// size.
    pub(crate) size: u64,
    /// path.
    pub(crate) path: String,
}

/// Upload Result payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UploadResult {
    /// file.
    pub(crate) file: Option<UploadedAttachment>,
    /// error.
    pub(crate) error: Option<String>,
}

/// Uploaded attachment discriminator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum UploadedAttachment {
    /// File retained in the server upload directory.
    UploadedFile(UploadedFile),
}

/// File version copied from Paseo's discriminated union.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub(crate) enum FileVersion {
    /// Current metadata.
    Ready {
        /// Workspace root.
        cwd: String,
        /// Relative path.
        path: String,
        /// Byte count.
        size: u64,
        /// ISO modification time.
        modified_at: String,
        /// High precision disk identity.
        #[serde(skip_serializing_if = "Option::is_none")]
        revision: Option<String>,
    },
    /// The file has disappeared.
    Missing {
        /// Workspace root.
        cwd: String,
        /// Relative path.
        path: String,
    },
    /// Inspection failure.
    Error {
        /// Workspace root.
        cwd: String,
        /// Relative path.
        path: String,
        /// Diagnostic.
        error: String,
    },
}

/// Atomic edit outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub(crate) enum WriteOutcome {
    /// Successfully persisted edit.
    Written {
        /// Modification timestamp.
        modified_at: String,
        /// Byte count.
        size: u64,
        /// Disk identity.
        #[serde(skip_serializing_if = "Option::is_none")]
        revision: Option<String>,
    },
    /// The expected revision is no longer current.
    Conflict {
        /// Current disk state.
        version: FileVersion,
    },
    /// The edit was rejected.
    Error {
        /// Diagnostic.
        error: String,
    },
}

#[cfg(test)]
mod tests;
