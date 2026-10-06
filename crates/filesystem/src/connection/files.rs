//! Connection-owned file observers and transfers.

/// Client methods implemented by this component; consumed by capability discovery.
pub const METHODS: &[&str] = &[
    "directory.suggestions.request",
    "fs.explorer.request",
    "fs.file.subscribe.request",
    "fs.file.unsubscribe.request",
    "fs.file.write.request",
    "fs.entry.create.request",
    "fs.entry.rename.request",
    "fs.entry.duplicate.request",
    "fs.entry.delete.request",
    "fs.file.download_token.request",
    "file.upload.request",
];

use crate::rpc::files::project_version;
use crate::service::files as port;
use model::ErrorCode;
use serde::Serialize;
use serde_json::Value;

use crate::dispatch::State as Shared;
mod connection;
/// Binary preview and shared download chunk reading.
pub mod transfer;
pub use connection::FileConnection;
pub(crate) use connection::FileRequest;

async fn execute(method: String, params: Value, state: &Shared) -> Result<Value, ErrorCode> {
    state
        .run(state.files.clone(), ErrorCode::ProjectIo, move |files| {
            crate::rpc::files::dispatch(files, &method, params).map_err(Into::into)
        })
        .await
}

fn decode<T: serde::de::DeserializeOwned>(params: Value) -> Result<T, ErrorCode> {
    serde_json::from_value(params).map_err(|_| ErrorCode::InvalidMessage)
}

fn encode(value: impl Serialize) -> Result<Value, ErrorCode> {
    serde_json::to_value(value).map_err(|_| ErrorCode::ProjectIo)
}
