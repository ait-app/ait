//! Connection-owned observers and request integration.
//!
//! Connection-owned file observers and transfers.

/// Client methods implemented by this component; consumed by capability discovery.
pub const METHODS: &[MethodSpec] = &[
    MethodSpec::request("directory.suggestions.request"),
    MethodSpec::request("fs.explorer.request"),
    MethodSpec::request("fs.file.subscribe.request"),
    MethodSpec::request("fs.file.unsubscribe.request"),
    MethodSpec::request("fs.file.write.request"),
    MethodSpec::request("fs.entry.create.request"),
    MethodSpec::request("fs.entry.rename.request"),
    MethodSpec::request("fs.entry.duplicate.request"),
    MethodSpec::request("fs.entry.delete.request"),
    MethodSpec::request("fs.file.download_token.request"),
    MethodSpec::request("file.upload.request"),
];

use crate::files::ports as port;
use crate::files::rpc::project_version;
use model::ErrorCode;
use model::methods::MethodSpec;
use serde_json::Value;

use crate::dispatch::State as Shared;
mod state;
/// Binary preview and shared download chunk reading.
pub mod transfer;
pub(crate) use state::FileConnection;
pub(crate) use state::FileRequest;

async fn execute(method: String, params: Value, state: &Shared) -> Result<Value, ErrorCode> {
    state
        .run(state.files.clone(), ErrorCode::ProjectIo, move |files| {
            crate::files::rpc::dispatch(files, &method, params).map_err(Into::into)
        })
        .await
}
