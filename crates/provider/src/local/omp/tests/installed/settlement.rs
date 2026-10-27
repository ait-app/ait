//! Fault injection around the official peer, retaining isolated configuration and loopback inference.
use super::*;

/// Verify the installed peer still settles through native state when its notification is lost.
pub(super) async fn verify(client: &OmpClient, spec: &AgentSessionSpec) {
    let mut client = client.clone();
    let mut environment = client
        .environment
        .entries()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect::<BTreeMap<_, _>>();
    environment.insert(
        "AIT_TEST_OMP_REAL_BIN".to_owned(),
        client.program.to_str().unwrap().to_owned(),
    );
    client.environment = AgentEnvironment::try_from(environment).unwrap();
    let proxy = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/local/omp/tests/fixtures/settlement_proxy.py");
    client.program = proxy;
    let mut session = client.create_session(spec).await.unwrap();
    for prompt in [
        "Persist this isolated settlement test.",
        "Continue this isolated settlement test.",
    ] {
        session.start_turn(prompt, &spec.config).await.unwrap();
        let events = complete(session.as_mut()).await;
        assert!(
            matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
            "{events:?}: {:?}",
            session.failure_message()
        );
        assert_eq!(session.failure_message(), None);
    }
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let history = client.history(&handle, &spec.cwd).await.unwrap();
    assert_eq!(
        history
            .iter()
            .filter(|entry| entry.item["type"] == "user_message")
            .count(),
        2
    );
}
