//! Model discovery must not depend on creating a usable default Agent.
use serde_json::{Value, json};

use super::{config::Selection, runtime::Runtime};
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
        let mut permissions = runtime
            .api
            .call("permissionPresets/catalog", json!({}))
            .await?;
        // The default is informational; omitted draft values still inherit native configuration.
        permissions["currentValue"] = permissions["defaultPreset"].clone();
        let details = Selection {
            presets: runtime.api.call("agentPresets/list", json!({})).await?,
            preset: None,
            catalog,
            permissions,
            model: Value::Null,
        }
        .details()?;
        Ok(details)
    }
    .await;
    let closed = runtime.close().await;
    closed?;
    result
}
