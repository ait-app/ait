//! Typed, provider-owned HTML export results, without exposing native staging paths.

/// Maximum native artifact size, shared with the authenticated download service.
pub const MAX_NATIVE_HTML_EXPORT_BYTES: u64 = model::download::MAX_ARTIFACT_BYTES;

/// An adopted native HTML file, positioned at its beginning and owned by the caller.
#[derive(Debug)]
pub struct NativeHtmlExport {
    /// Anonymous owned file containing the exact provider-rendered bytes.
    pub file: std::fs::File,
    /// Suggested download filename, never a filesystem path.
    pub file_name: String,
    /// Complete file size in bytes.
    pub size: u64,
}

/// Native export failure, without paths or native payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeHtmlExportError {
    /// The native renderer refused the request or returned inconsistent facts.
    Rejected,
    /// The artifact exceeds the download service's size bound.
    TooLarge,
    /// All bounded background copy slots are currently occupied.
    Busy,
    /// Staging, reading or native communication failed.
    Io,
}
