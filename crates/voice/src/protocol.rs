//! Canonical voice methods; payload fields retain Paseo's camelCase spelling.

use serde::Deserialize;

/// Enable voice for a specific Agent, or disable the current connection's voice mode.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Mode {
    /// Desired mode.
    pub(crate) enabled: bool,
    /// Required when enabling; resolved to one canonical Agent ID.
    pub(crate) agent_id: Option<String>,
}

/// Base64 microphone audio with an optional utterance boundary.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct VoiceChunk {
    /// Base64 bytes.
    pub(crate) audio: String,
    /// Audio MIME type, including PCM sample rate where applicable.
    pub(crate) format: String,
    /// Explicit end of utterance; PCM also supports silence-based boundaries.
    pub(crate) is_last: bool,
}

/// Confirm one server audio chunk has actually finished playing.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Played {
    /// Audio chunk ID, scoped to this physical connection.
    pub(crate) id: String,
}

/// Start or acknowledge an existing stream on the same connection.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Start {
    /// Connection-local stream ID.
    pub(crate) dictation_id: String,
    /// Immutable format for the entire stream.
    pub(crate) format: String,
}

/// A sequenced, replayable audio chunk.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Chunk {
    /// Connection-local stream ID.
    #[expect(
        dead_code,
        reason = "routing reads dictationId before decoding; deny_unknown_fields still accepts it"
    )]
    pub(crate) dictation_id: String,
    /// Zero-based sequence number.
    pub(crate) seq: u32,
    /// Base64 bytes.
    pub(crate) audio: String,
    /// Must match the stream's start format.
    pub(crate) format: String,
}

/// Finish after every chunk through `final_seq` has arrived.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Finish {
    /// Connection-local stream ID.
    #[expect(
        dead_code,
        reason = "routing reads dictationId before decoding; deny_unknown_fields still accepts it"
    )]
    dictation_id: String,
    /// Inclusive final sequence number.
    pub(crate) final_seq: u32,
}

/// Cancel a stream and any outstanding transcription work.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Cancel {
    /// Connection-local stream ID.
    #[expect(
        dead_code,
        reason = "routing reads dictationId before decoding; deny_unknown_fields still accepts it"
    )]
    dictation_id: String,
}

pub(crate) fn decode<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
) -> Result<T, crate::Error> {
    serde_json::from_value(value).map_err(|_| crate::Error::Invalid)
}
