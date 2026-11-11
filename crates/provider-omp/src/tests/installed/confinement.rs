//! The official peer under write confinement (ADR-215): it keeps writing its session state
//! while its configuration stays read-only.
use super::images::installed_with_recording_model;
use super::*;

#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; runs loopback-only inference under Seatbelt"]
async fn installed_omp_runs_confined_without_changing_its_configuration() {
    let (fixture, requests) = installed_with_recording_model().await;
    let agent = fixture.root.path().join("agent");
    std::fs::create_dir_all(fixture.root.path().join(".git")).unwrap();
    // Without config.yml OMP migrates the legacy settings.json into it, which needs a write.
    std::fs::write(agent.join("settings.json"), "{\"steeringMode\":\"all\"}").unwrap();
    let models = std::fs::read_to_string(agent.join("models.yml")).unwrap();
    let spec = AgentSessionSpec {
        confinement: confinement(fixture.root.path()),
        ..fixture.spec.clone()
    };
    let mut session = fixture.client.create_session(&spec).await.unwrap();

    session.start_turn("hello", &spec.config).await.unwrap();
    let events = complete(session.as_mut()).await;
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();

    assert!(
        matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
        "{events:?}"
    );
    // OMP may also ask the model to summarize discovered skills; one request is the prompt.
    assert!(
        requests
            .lock()
            .unwrap()
            .iter()
            .any(|request| request["messages"].to_string().contains("hello"))
    );
    let history = fixture.client.history(&handle, &spec.cwd).await.unwrap();
    assert!(
        history
            .iter()
            .any(|entry| entry.item["text"] == "Fixture complete"),
        "{history:?}"
    );
    assert!(agent.join("agent.db").is_file());
    // The denied migration leaves both files as they were, and the session still ran.
    assert!(!agent.join("config.yml").exists());
    assert!(agent.join("settings.json").is_file());
    assert_eq!(
        std::fs::read_to_string(agent.join("models.yml")).unwrap(),
        models
    );
}
