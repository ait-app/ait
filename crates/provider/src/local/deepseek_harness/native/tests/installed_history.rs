use crate::{
    local::deepseek_harness::{DeepSeekHarnessClient, PROVIDER},
    ports::{
        agent_session::{AgentClient, AgentResumePurpose, AgentSessionSpec},
        native_history::{ListOptions, SessionHistory},
    },
};
use domain::agent_runtime::AgentPersistenceHandle;

async fn imported_existing() -> (
    DeepSeekHarnessClient,
    AgentPersistenceHandle,
    SessionHistory,
) {
    let binary = std::env::var_os("AIT_TEST_DSH_BIN").expect("set AIT_TEST_DSH_BIN");
    let id = std::env::var("AIT_TEST_DSH_SESSION_ID").expect("select an idle session explicitly");
    let client = DeepSeekHarnessClient::new(binary.into());
    let listed = client
        .list_sessions(&ListOptions {
            cwd: None,
            scan_limit: 4096,
        })
        .await
        .unwrap();
    let entry = listed
        .iter()
        .find(|entry| entry.provider_handle_id == id)
        .expect("session is listed");
    let mut handle = AgentPersistenceHandle {
        provider: PROVIDER.into(),
        session_id: id,
        native_handle: None,
        metadata: None,
    };
    let history = client.inspect_session(&handle, &entry.cwd).await.unwrap();
    assert!(!history.active, "do not attach to an active native turn");
    assert!(
        !history.entries.is_empty(),
        "exercise real persisted history"
    );
    assert!(history.descriptor.first_prompt_preview.is_some());
    handle.metadata = Some(history.resume_metadata.clone());
    (client, handle, history)
}

#[tokio::test]
#[ignore = "requires AIT_TEST_DSH_BIN and AIT_TEST_DSH_SESSION_ID; read-only existing history"]
async fn installed_existing_session_is_importable_without_writes() {
    let (client, handle, before) = imported_existing().await;
    let after = client
        .inspect_session(&handle, &before.descriptor.cwd)
        .await
        .unwrap();
    assert!(
        after.entries == before.entries,
        "history changed during import"
    );
    assert!(
        after.config == before.config,
        "native selections changed during import"
    );
}

#[tokio::test]
#[ignore = "requires AIT_TEST_DSH_BIN and an idle, released AIT_TEST_DSH_SESSION_ID; no prompt"]
async fn installed_existing_session_import_and_resume_preserve_history() {
    let (client, handle, before) = imported_existing().await;
    let spec = AgentSessionSpec {
        provider: PROVIDER.into(),
        cwd: before.descriptor.cwd.clone(),
        config: before.config.clone(),
    };
    let mut session = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert_eq!(session.persistence().unwrap().session_id, handle.session_id);
    let runtime = session.runtime_info().await.unwrap();
    assert_eq!(runtime.mode_id, before.config.mode_id);
    assert_eq!(runtime.model, before.config.model);
    session.close().await.unwrap();
    let after = client.inspect_session(&handle, &spec.cwd).await.unwrap();
    assert!(!after.active);
    assert!(
        after.entries == before.entries,
        "history changed without a prompt"
    );
    assert!(after.config == before.config, "native selections changed");
}
