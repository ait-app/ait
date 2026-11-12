use std::sync::Arc;

use super::*;
use domain::agent_runtime::AgentPersistenceHandle;
use provider_core::ports::agent_session::{
    AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionSpec,
};
use provider_core::ports::bot_session::{BotSessionSupport, InstructionDelivery};

mod concurrency;
mod diagnostics;
mod paseo;

#[derive(Debug)]
struct Client(bool);
#[async_trait::async_trait]
impl AgentClient for Client {
    fn provider(&self) -> &'static str {
        "codex"
    }
    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        Ok(self.0)
    }
    async fn discover(&self, _cwd: &str) -> Result<Details, AgentSessionError> {
        Ok(Details {
            models: vec![json!({"id":"test","provider":"codex","label":"Test"})],
            modes: vec![json!({"id":"read-only","label":"Read only"})],
            features: vec![],
        })
    }
    async fn create_session(
        &self,
        _spec: &AgentSessionSpec,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        Err(AgentSessionError::Unavailable)
    }
    async fn resume_session(
        &self,
        _handle: &AgentPersistenceHandle,
        _spec: &AgentSessionSpec,
        _purpose: AgentResumePurpose,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        Err(AgentSessionError::Unavailable)
    }
}

/// An uninstalled adapter that still imports native sessions and hosts Bot sessions natively.
#[derive(Debug)]
struct NativeClient;
#[async_trait::async_trait]
impl AgentClient for NativeClient {
    fn provider(&self) -> &'static str {
        "claude"
    }
    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        Ok(false)
    }
    fn bot_session_support(&self) -> BotSessionSupport {
        BotSessionSupport {
            instructions: InstructionDelivery::SystemPrompt,
            read_only_mode: Some("read-only"),
            tool_preapproval: true,
            ait_tools: true,
        }
    }
    fn supports_session_import(&self) -> bool {
        true
    }
    async fn create_session(
        &self,
        _spec: &AgentSessionSpec,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        Err(AgentSessionError::Unavailable)
    }
    async fn resume_session(
        &self,
        _handle: &AgentPersistenceHandle,
        _spec: &AgentSessionSpec,
        _purpose: AgentResumePurpose,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        Err(AgentSessionError::Unavailable)
    }
}

#[tokio::test]
async fn catalog_uses_registered_adapters_scopes_cache_and_honors_content_hashes() {
    let clients: BTreeMap<String, Arc<dyn AgentClient>> = BTreeMap::from([(
        "codex".to_owned(),
        Arc::new(Client(true)) as Arc<dyn AgentClient>,
    )]);
    let catalog = Catalog::default();
    let events = SessionEvents::default();
    let cwd = tempfile::tempdir().unwrap();
    let first = catalog
        .execute(
            &clients,
            &events,
            "provider.snapshot.get.request",
            json!({"cwd":cwd.path()}),
        )
        .await
        .unwrap();
    assert_eq!(first["entries"][0]["status"], "ready");
    let unchanged = catalog
        .execute(
            &clients,
            &events,
            "provider.snapshot.get.request",
            json!({"cwd":cwd.path(),"ifNoneMatch":first["snapshotHash"]}),
        )
        .await
        .unwrap();
    assert_eq!(unchanged["notModified"], true);
    assert_eq!(unchanged["entries"], json!([]));
    for (method, field) in [
        ("provider.models.list.request", "models"),
        ("provider.modes.list.request", "modes"),
        ("provider.features.list.request", "features"),
    ] {
        let value = catalog
            .execute(
                &clients,
                &events,
                method,
                json!({"provider":"codex","cwd":cwd.path()}),
            )
            .await
            .unwrap();
        assert!(value[field].is_array());
    }
    assert_eq!(
        catalog
            .execute(
                &clients,
                &events,
                "provider.snapshot.refresh.request",
                json!({"cwd":cwd.path(),"providers":["codex"]})
            )
            .await
            .unwrap()["acknowledged"],
        true
    );
    assert!(
        catalog
            .execute(
                &clients,
                &events,
                "provider.snapshot.refresh.request",
                json!({"providers":["missing"]})
            )
            .await
            .is_err()
    );
    assert!(
        catalog
            .execute(
                &clients,
                &events,
                "provider.models.list.request",
                json!({"provider":"missing"})
            )
            .await
            .is_err()
    );
    assert!(
        catalog
            .execute(
                &clients,
                &events,
                "provider.snapshot.get.request",
                json!({"cwd":"relative"})
            )
            .await
            .is_err()
    );
    assert!(
        catalog
            .execute(
                &clients,
                &events,
                "provider.snapshot.get.request",
                json!({"cwd":cwd.path().join("missing")})
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn unavailable_provider_is_reported_truthfully() {
    let clients: BTreeMap<String, Arc<dyn AgentClient>> = BTreeMap::from([(
        "codex".to_owned(),
        Arc::new(Client(false)) as Arc<dyn AgentClient>,
    )]);
    let value = Catalog::default()
        .execute(
            &clients,
            &SessionEvents::default(),
            "provider.available.list.request",
            json!({}),
        )
        .await
        .unwrap();
    assert_eq!(value["providers"][0]["available"], false);
    assert_eq!(
        value["providers"][0]["error"],
        "Provider executable is unavailable"
    );
}

#[tokio::test]
async fn native_import_support_does_not_require_an_installed_executable() {
    assert_eq!(
        discover(&Client(true), "/tmp").await.value["supportsSessionImport"],
        false
    );
    let entry = discover(&NativeClient, "/tmp").await;
    assert_eq!(entry.value["supportsSessionImport"], true);
    assert_eq!(entry.value["status"], "unavailable");
}

#[tokio::test]
async fn snapshot_entries_publish_bot_session_support() {
    let clients: BTreeMap<String, Arc<dyn AgentClient>> = BTreeMap::from([(
        "codex".to_owned(),
        Arc::new(Client(true)) as Arc<dyn AgentClient>,
    )]);
    let cwd = tempfile::tempdir().unwrap();

    let snapshot = Catalog::default()
        .execute(
            &clients,
            &SessionEvents::default(),
            "provider.snapshot.get.request",
            json!({"cwd":cwd.path()}),
        )
        .await
        .unwrap();

    assert_eq!(
        snapshot["entries"][0]["botSession"],
        json!({"instructions":"turn_prefix","readOnlyMode":null,
            "toolPreapproval":false,"aitTools":false})
    );
    let entry = discover(&NativeClient, cwd.path().to_str().unwrap()).await;
    assert_eq!(
        entry.value["botSession"],
        json!({"instructions":"system_prompt","readOnlyMode":"read-only",
            "toolPreapproval":true,"aitTools":true})
    );
}
