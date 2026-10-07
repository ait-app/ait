//! Model discovery must not depend on creating a usable default Agent.
use serde_json::{Value, json};

use super::{config::Selection, modes, runtime::Runtime};
use crate::{
    local::deepseek_harness::DeepSeekHarnessClient, ports::agent_session::AgentSessionError,
    protocol::provider::Details,
};

/// Read the native catalog without creating a session, selecting a model or sending input.
/// Returns discovery/transport errors; session-specific permissions are checked on creation.
pub(in crate::local::deepseek_harness) async fn discover(
    client: &DeepSeekHarnessClient,
    cwd: &str,
) -> Result<Details, AgentSessionError> {
    let mut runtime = Runtime::open(client, cwd).await?;
    let result = async {
        let catalog = runtime.api.call("session/modelCatalog", json!({})).await?;
        let mut details = Selection {
            catalog,
            permissions: json!({"options":[]}),
            model: Value::Null,
        }
        .details()?;
        details
            .modes
            .clone_from(modes().as_array().expect("built-in modes are an array"));
        Ok(details)
    }
    .await;
    let closed = runtime.close().await;
    closed?;
    result
}
