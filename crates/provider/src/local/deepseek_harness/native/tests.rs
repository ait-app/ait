use crate::{
    local::deepseek_harness::{DeepSeekHarnessClient, PROVIDER},
    ports::{
        agent_session::{AgentClient, AgentResumePurpose, AgentSessionSpec},
        environment::AgentEnvironment,
    },
};
use domain::agent_runtime::StoredAgentConfig;
use std::collections::BTreeMap;

mod fixture;
mod interactions;
mod session;
mod validation;

#[tokio::test]
#[ignore = "requires an installed DSH CLI via AIT_TEST_DSH_BIN; no model request"]
async fn installed_host_discovers_switches_permissions_and_adopts_legacy_sessions() {
    let binary = std::env::var_os("AIT_TEST_DSH_BIN").expect("set AIT_TEST_DSH_BIN");
    let directory = tempfile::tempdir().unwrap();
    let mut client = DeepSeekHarnessClient::new(binary.into());
    client.environment = AgentEnvironment::try_from(BTreeMap::from([(
        "DSH_HOME".into(),
        directory.path().to_str().unwrap().into(),
    )]))
    .unwrap();
    let mut spec = AgentSessionSpec {
        provider: PROVIDER.into(),
        cwd: directory.path().to_str().unwrap().into(),
        config: StoredAgentConfig::default(),
    };
    let details = client.discover(&spec.cwd).await.unwrap();
    assert!(!details.models.is_empty());
    assert!(details.modes.iter().any(|mode| mode["id"] == "read-only"));
    spec.config.mode_id = Some("read-only".into());
    let mut session = client.create_session(&spec).await.unwrap();
    assert_eq!(
        session.runtime_info().await.unwrap().mode_id.as_deref(),
        Some("read-only")
    );
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let mut resumed = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert_eq!(resumed.persistence().unwrap().session_id, handle.session_id);
    resumed.close().await.unwrap();
    spec.config.mode_id = None;
    let legacy_client = client.clone().with_acp_profile();
    let mut legacy = legacy_client.create_session(&spec).await.unwrap();
    let legacy_handle = legacy.persistence().unwrap();
    legacy.close().await.unwrap();
    let mut adopted = client
        .resume_session(&legacy_handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert_eq!(
        adopted.persistence().unwrap().session_id,
        legacy_handle.session_id
    );
    adopted.close().await.unwrap();
}
