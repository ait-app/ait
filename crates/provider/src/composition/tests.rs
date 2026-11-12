use super::*;

#[cfg(unix)]
use crate::service::summary_generation::test_config::Configuration;
use domain::agent_runtime::AgentPersistenceHandle;
#[cfg(unix)]
use domain::summary::{SummaryKind, SummaryRequest};
use provider_core::ports::agent_session::{
    AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionSpec,
};
#[cfg(unix)]
use serde_json::json;

/// An adapter from another crate: the container accepts any cloneable client.
#[derive(Debug, Clone)]
struct External;

#[async_trait::async_trait]
impl AgentClient for External {
    fn provider(&self) -> &'static str {
        "external"
    }

    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        Ok(false)
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

#[test]
fn external_adapters_join_the_assembly_with_the_shared_image_directory() {
    let root = tempfile::tempdir().unwrap();
    let assembled = Providers::configured(root.path(), |_| None).clients.len();
    let mut received = None;
    let providers = Providers::configured(root.path(), |_| None).with(|images| {
        received = Some(images.to_owned());
        External
    });

    assert_eq!(
        received.unwrap(),
        root.path().join("agents/provider-images")
    );
    assert_eq!(providers.clients.len(), assembled + 1);
    assert_eq!(providers.clients.last().unwrap().provider(), "external");
    assert!(!root.path().join("agents").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn summary_generators_record_their_calls_in_the_installed_host_ledger() {
    use crate::ports::usage_ledger::{
        CallOutcome, CallSource, UsageGroup, UsageLedger, UsageQuery,
    };

    let fixture = crate::test_support::Fixture::new();
    let data = tempfile::tempdir().unwrap();
    let ledger = Arc::new(SqliteUsageLedger::open(data.path()).unwrap());
    let config = Arc::new(Configuration::default());
    config
        .patch(&json!({"metadataGeneration":{"providers":[{"provider":"codex","model":"m"}]}}))
        .unwrap();
    let generator = Providers::configured(fixture.root.path(), |name| {
        Some(if name == "AIT_SERVER_CODEX_BIN" {
            fixture.program.clone().into_os_string()
        } else {
            fixture.root.path().join(name).into_os_string()
        })
    })
    .with_usage_ledger(ledger.clone())
    .summary_generator(config);
    std::fs::write(
        fixture.cwd.join("metadata-response.json"),
        r#"{"title":"Recorded title"}"#,
    )
    .unwrap();

    generator
        .generate(SummaryRequest {
            kind: SummaryKind::Title,
            cwd: fixture.cwd.to_str().unwrap().to_owned(),
            context: "Record the generation".into(),
            selection: None,
        })
        .await
        .unwrap();
    let summary = ledger
        .summarize(UsageQuery {
            from: 0,
            to: chrono::Utc::now().timestamp_millis() + 1,
            group_by: vec![UsageGroup::Source, UsageGroup::Outcome],
        })
        .await
        .unwrap();

    let rows: Vec<_> = summary
        .rows
        .iter()
        .map(|row| (row.key.source, row.key.outcome, row.calls))
        .collect();
    assert_eq!(
        rows,
        [(Some(CallSource::Summary), Some(CallOutcome::Completed), 1)]
    );
}
