use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::Utc;
use model::ErrorCode;
use serde_json::{Value, json};

use crate::ports::agent_session::{AgentClient, AgentSessionError, AgentSessionSpec};
use crate::protocol::provider::FeaturesRequest;
use crate::service::presets::PresetService;

pub(crate) async fn features(
    clients: &BTreeMap<String, Arc<dyn AgentClient>>,
    presets: Option<&PresetService>,
    request: FeaturesRequest,
) -> Result<Value, ErrorCode> {
    let mut config = request.draft_config;
    let mut response = json!({"provider":config.provider,"fetchedAt":Utc::now().to_rfc3339()});
    let directory = super::scope::directory(&config.cwd);
    let error = if let Ok(cwd) = directory {
        if let Some(client) = clients.get(&config.provider) {
            config.stored.model = config.stored.model.and_then(|model| {
                let model = model.trim();
                (!model.is_empty() && model != "default").then(|| model.to_owned())
            });
            match client
                .draft_features(&AgentSessionSpec {
                    provider: config.provider.clone(),
                    cwd,
                    config: config.stored.clone(),
                    confinement: None,
                })
                .await
            {
                Ok(mut features) => {
                    // A draft Agent has no snapshot to merge presets into, so the same store the
                    // live composer reads is merged here. `apply` only touches daemon config or
                    // an already-present native feature, so this adds no native probe.
                    if let Some(presets) = presets {
                        presets.apply(
                            &config.provider,
                            client.owns_native_presets(),
                            &mut features,
                            &config.stored,
                        );
                    }
                    response["features"] = json!(features);
                    None
                }
                Err(AgentSessionError::Unavailable) => Some("Provider executable is unavailable"),
                Err(_) => Some("Provider feature discovery failed"),
            }
        } else {
            Some("Provider is not installed")
        }
    } else {
        Some("Working directory is unavailable")
    };
    response["error"] = json!(error);
    crate::rpc::timeline::bounded(response)
}
