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
mod imports;
mod interactions;
mod recovery;
mod session;
mod sources;
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
    assert!(client.history(&handle, &spec.cwd).await.unwrap().is_empty());
    let listed = client
        .list_sessions(&crate::ports::native_history::ListOptions {
            cwd: Some(spec.cwd.clone()),
            scan_limit: 20,
        })
        .await
        .unwrap();
    // Cold sessions without cached list metadata can remain visible in DSH 0.1.5.
    assert!(listed.iter().all(|entry| entry.cwd == spec.cwd));
    let mut imported_handle = handle.clone();
    imported_handle.metadata = None;
    let imported = client
        .inspect_session(&imported_handle, &spec.cwd)
        .await
        .unwrap();
    assert!(imported.entries.is_empty());
    assert!(!imported.active);
    assert_eq!(imported.config.mode_id.as_deref(), Some("read-only"));
    imported_handle.metadata = Some(imported.resume_metadata);
    let mut adopted_import = client
        .resume_session(
            &imported_handle,
            &AgentSessionSpec {
                config: imported.config,
                ..spec.clone()
            },
            AgentResumePurpose::Interactive,
        )
        .await
        .unwrap();
    assert_eq!(
        adopted_import.persistence().unwrap().session_id,
        handle.session_id
    );
    adopted_import.close().await.unwrap();
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
