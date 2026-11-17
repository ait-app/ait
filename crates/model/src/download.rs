//! Owned generated content handed from a producer to the existing download-grant service.

/// A completed private artifact. The producer creates an anonymous temporary file, so dropping
/// this value or its transferred reader closes and removes the artifact without a public path.
#[derive(Debug)]
pub struct GeneratedArtifact {
    /// Agent whose persisted content was selected; used to authorize explicit release.
    pub agent_id: String,
    /// Safe basename advertised to the client.
    pub file_name: String,
    /// Content type chosen by the typed producer.
    pub mime_type: String,
    /// Exact completed byte count, bounded by the producer and grant service.
    pub size: u64,
    /// Rewound, immutable-by-convention anonymous file owned by the download lifetime.
    pub file: std::fs::File,
}

/// Largest generated export accepted by both producer and downloader.
pub const MAX_ARTIFACT_BYTES: u64 = 16 * 1024 * 1024;
