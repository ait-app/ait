//! Opt-in smoke test against authenticated local AGY; no tools are requested.

use super::*;

async fn answer(session: &mut dyn AgentSession) -> String {
    tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            match session.poll_turn().unwrap() {
                Some(AgentTurnEvent::Completed(Some(text))) => return text,
                Some(AgentTurnEvent::Failed | AgentTurnEvent::Cancelled) => {
                    panic!("native turn failed")
                }
                _ => tokio::time::sleep(Duration::from_millis(10)).await,
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "requires authenticated AGY and explicitly consumes three model turns"]
async fn authenticated_multi_turn_and_resume() {
    let client = AntigravityClient::installed();
    assert!(client.is_available().await.unwrap());
    let directory = tempfile::tempdir().unwrap();
    let cwd = directory
        .path()
        .canonicalize()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let models = client.discover(&cwd).await.unwrap().models;
    let model = models
        .iter()
        .find(|model| model["id"].as_str().is_some_and(|id| id.ends_with("-low")))
        .unwrap_or(&models[0])["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let spec = AgentSessionSpec {
        provider: PROVIDER.to_owned(),
        cwd,
        config: StoredAgentConfig {
            model: Some(model),
            mode_id: Some("plan".to_owned()),
            ..Default::default()
        },
    };
    let mut session = client.create_session(&spec).await.unwrap();
    let handle = session.persistence().unwrap();
    session
        .start_turn(
            "Remember the code agy-provider-smoke. Reply with exactly that code. Do not use tools.",
            &spec.config,
        )
        .await
        .unwrap();
    assert!(
        answer(session.as_mut())
            .await
            .contains("agy-provider-smoke")
    );
    session
        .start_turn(
            "Repeat the code I asked you to remember. Nothing else. Do not use tools.",
            &spec.config,
        )
        .await
        .unwrap();
    assert!(
        answer(session.as_mut())
            .await
            .contains("agy-provider-smoke")
    );
    session.close().await.unwrap();
    let mut resumed = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    resumed
        .start_turn(
            "Repeat the code I asked you to remember. Nothing else. Do not use tools.",
            &spec.config,
        )
        .await
        .unwrap();
    assert!(
        answer(resumed.as_mut())
            .await
            .contains("agy-provider-smoke")
    );
    assert_eq!(resumed.persistence().unwrap(), handle);
    resumed.close().await.unwrap();
}
