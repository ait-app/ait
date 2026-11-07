use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{Value, json};

use super::super::draft;
use crate::ports::agent_session::{AgentClient, AgentSessionError, AgentSessionSpec};
use crate::protocol::provider::{Details, FeaturesRequest};
use crate::service::presets::PresetService;
use persistence::storage::daemon_config::FileDaemonConfigStore;

/// Adapter that answers `draft_features` with a fixed list and declares preset ownership.
#[derive(Debug)]
struct Stub {
    provider: &'static str,
    native: bool,
    features: Vec<Value>,
}

#[async_trait::async_trait]
impl AgentClient for Stub {
    fn provider(&self) -> &'static str {
        self.provider
    }
    fn owns_native_presets(&self) -> bool {
        self.native
    }
    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        Ok(true)
    }
    async fn discover(&self, _cwd: &str) -> Result<Details, AgentSessionError> {
        Ok(Details {
            models: Vec::new(),
            modes: Vec::new(),
            features: Vec::new(),
        })
    }
    async fn draft_features(
        &self,
        _spec: &AgentSessionSpec,
    ) -> Result<Vec<Value>, AgentSessionError> {
        Ok(self.features.clone())
    }
    async fn create_session(
        &self,
        _spec: &AgentSessionSpec,
    ) -> Result<Box<dyn crate::ports::agent_session::AgentSession>, AgentSessionError> {
        Err(AgentSessionError::Unavailable)
    }
    async fn resume_session(
        &self,
        _handle: &domain::agent_runtime::AgentPersistenceHandle,
        _spec: &AgentSessionSpec,
        _purpose: crate::ports::agent_session::AgentResumePurpose,
    ) -> Result<Box<dyn crate::ports::agent_session::AgentSession>, AgentSessionError> {
        Err(AgentSessionError::Unavailable)
    }
}

/// One draft feature request for `provider` in `cwd`.
fn request(provider: &str, cwd: &str) -> FeaturesRequest {
    FeaturesRequest {
        draft_config: crate::protocol::agent_execution::SessionConfig {
            provider: provider.to_owned(),
            cwd: cwd.to_owned(),
            title: None,
            stored: domain::agent_runtime::StoredAgentConfig::default(),
        },
    }
}

fn stub(provider: &'static str, native: bool, features: Vec<Value>) -> Arc<dyn AgentClient> {
    Arc::new(Stub {
        provider,
        native,
        features,
    }) as Arc<dyn AgentClient>
}

fn service(root: &tempfile::TempDir) -> PresetService {
    PresetService::new(Arc::new(FileDaemonConfigStore::with_defaults(
        root.path().join("config.json"),
    )))
}

fn presets(response: &Value) -> Vec<Value> {
    response["features"].as_array().cloned().unwrap_or_default()
}

#[tokio::test]
async fn a_managed_store_reaches_the_draft_feature_list() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().to_str().unwrap();
    let presets_service = service(&root);
    presets_service
        .save(
            "claude",
            cwd,
            None,
            &crate::ports::preset::Preset {
                name: "team".to_owned(),
                model: "claude-opus-5".to_owned(),
                thinking_option_id: Some("high".to_owned()),
                feature_values: BTreeMap::from([(
                    "modelRole.advisor".to_owned(),
                    json!("claude-sonnet-5"),
                )]),
            },
        )
        .await
        .unwrap();
    let clients = BTreeMap::from([(
        "claude".to_owned(),
        stub("claude", false, vec![json!({"id":"mode","type":"select"})]),
    )]);

    let response = draft::features(&clients, Some(&presets_service), request("claude", cwd))
        .await
        .unwrap();
    assert_eq!(response["error"], Value::Null);
    let features = presets(&response);
    assert_eq!(features[0]["id"], "modelPreset");
    let options = features[0]["options"].as_array().unwrap();
    let team = options
        .iter()
        .find(|option| option["id"] == "team")
        .expect("the stored preset is an option");
    // The composer reads this configuration to apply the whole selection in one step.
    assert_eq!(
        team["metadata"]["configuration"]["modelId"],
        "claude-opus-5"
    );
    assert_eq!(
        team["metadata"]["configuration"]["thinkingOptionId"],
        "high"
    );
    assert_eq!(
        team["metadata"]["configuration"]["featureValues"]["modelRole.advisor"],
        "claude-sonnet-5"
    );
    // The provider's own features survive behind the select.
    assert_eq!(features[1]["id"], "mode");
}

#[tokio::test]
async fn a_managed_store_without_presets_leaves_the_list_to_the_client_empty_state() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().to_str().unwrap();
    let clients = BTreeMap::from([(
        "claude".to_owned(),
        stub("claude", false, vec![json!({"id":"mode","type":"select"})]),
    )]);

    let response = draft::features(&clients, Some(&service(&root)), request("claude", cwd))
        .await
        .unwrap();
    let features = presets(&response);
    assert_eq!(features.len(), 1);
    assert_eq!(features[0]["id"], "mode");
}

#[tokio::test]
async fn a_native_store_keeps_its_own_presets_and_gains_no_ait_entries() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().to_str().unwrap();
    let presets_service = service(&root);
    // Ait also holds presets for this provider; a native store must not see them.
    presets_service
        .save(
            "omp",
            cwd,
            None,
            &crate::ports::preset::Preset {
                name: "ait_only".to_owned(),
                model: "futureppo/model".to_owned(),
                thinking_option_id: None,
                feature_values: BTreeMap::new(),
            },
        )
        .await
        .unwrap();
    let native = json!({
        "type":"select","id":"modelPreset","label":"OMP model preset","value":"real_work",
        "options":[{"id":"real_work","label":"real_work"}]
    });
    let clients = BTreeMap::from([(
        "omp".to_owned(),
        stub("omp", true, vec![native.clone(), json!({"id":"autoRetry"})]),
    )]);

    let response = draft::features(&clients, Some(&presets_service), request("omp", cwd))
        .await
        .unwrap();
    let features = presets(&response);
    assert_eq!(
        features[0], native,
        "the native select is preserved verbatim"
    );
    let names: Vec<&str> = features
        .iter()
        .filter_map(|feature| feature["id"].as_str())
        .collect();
    assert_eq!(names, ["modelPreset", "autoRetry"]);

    // Without Ait presets the native list is still exactly what the adapter reported.
    let empty_ait = tempfile::tempdir().unwrap();
    let response = draft::features(&clients, Some(&service(&empty_ait)), request("omp", cwd))
        .await
        .unwrap();
    assert_eq!(presets(&response)[0], native);
}
