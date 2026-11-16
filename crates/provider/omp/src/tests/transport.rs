//! Process-level transport behaviour: handshake, request timeouts and shutdown.
use super::*;
use crate::transport::Transport;

async fn turn_completes(client: &OmpClient, spec: &AgentSessionSpec) {
    let mut session = client.create_session(spec).await.unwrap();
    session.start_turn("hello", &spec.config).await.unwrap();
    assert!(matches!(
        complete(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn a_ready_without_matching_v2_frame_sizes_stays_on_protocol_v1() {
    for mode in ["v1", "sizes"] {
        let (root, client, spec) = fixture_with(&[("OMP_FIXTURE_READY", mode)]);

        turn_completes(&client, &spec).await;

        let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
        assert!(!calls.contains("negotiate_protocol"), "{mode}");
    }
}

#[tokio::test]
async fn frames_before_ready_are_ignored_and_v2_is_negotiated() {
    let (root, client, spec) = fixture_with(&[("OMP_FIXTURE_READY", "late")]);

    turn_completes(&client, &spec).await;

    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(calls.contains("negotiate_protocol"));
}

#[tokio::test]
async fn a_missing_ready_frame_times_out() {
    let (_root, mut client, spec) = fixture_with(&[("OMP_FIXTURE_READY", "never")]);
    client.timeouts.ready = Duration::from_millis(200);

    let started = std::time::Instant::now();
    let result = client.create_session(&spec).await;

    assert!(matches!(result, Err(AgentSessionError::Failed)));
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn model_discovery_has_no_request_timeout_but_other_requests_do() {
    let (_root, mut client, spec) = fixture_with(&[("OMP_FIXTURE_SLOW", "get_available_models")]);
    client.timeouts.request = Duration::from_millis(100);
    assert_eq!(client.discover(&spec.cwd).await.unwrap().models.len(), 4);

    let (_root, mut client, spec) = fixture_with(&[("OMP_FIXTURE_SLOW", "get_state")]);
    client.timeouts.request = Duration::from_millis(100);
    assert_eq!(
        client.discover(&spec.cwd).await.unwrap_err(),
        AgentSessionError::Failed
    );
}

#[tokio::test]
async fn prompts_over_one_mebibyte_reach_omp_and_return_in_chunks() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let prompt = AgentPrompt {
        text: "large image".to_owned(),
        images: vec![PromptImage {
            data: "A".repeat(2 * 1024 * 1024),
            mime_type: "image/png".to_owned(),
        }],
        ..AgentPrompt::default()
    };

    session.start_input(&prompt, &spec.config).await.unwrap();

    assert!(matches!(
        complete(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn an_outbound_line_over_the_cap_is_rejected_before_writing() {
    let (_root, client, spec) = fixture();
    let mut transport = Transport::spawn(&client, &spec.cwd, &[], None, client.timeouts).unwrap();
    transport.initialize().await.unwrap();

    let oversized = json!({"type":"get_state","padding":"x".repeat(64 * 1024 * 1024)});
    assert_eq!(
        transport.send(&oversized).await,
        Err(AgentSessionError::Rejected)
    );
    assert!(transport.request("get_state", json!({})).await.is_ok());
    transport.close().await.unwrap();
}

#[tokio::test]
async fn close_ends_stdin_then_terminates_the_process_group() {
    let (root, client, spec) = fixture_with(&[("OMP_FIXTURE_SHUTDOWN", "term")]);
    let mut session = client.create_session(&spec).await.unwrap();

    let started = std::time::Instant::now();
    session.close().await.unwrap();

    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(root.path().join("agent/sigterm").exists());
    assert_eq!(session.failure_message(), None);
}

#[tokio::test]
async fn close_kills_a_process_that_ignores_sigterm() {
    let (_root, client, spec) = fixture_with(&[("OMP_FIXTURE_SHUTDOWN", "stubborn")]);
    let mut session = client.create_session(&spec).await.unwrap();

    let started = std::time::Instant::now();
    session.close().await.unwrap();

    assert!(started.elapsed() >= Duration::from_secs(2));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(session.failure_message(), None);
}

#[tokio::test]
async fn a_failed_background_write_poisons_the_connection() {
    let (_root, client, spec) = fixture();
    // A process that closed its stdin refuses every write.
    let client = OmpClient {
        program: PathBuf::from("/bin/sh"),
        ..client
    };
    let script = r#"exec 0<&-; echo '{"type":"stdin_closed"}'; exec sleep 30"#;
    let args = ["-c".to_owned(), script.to_owned()];
    let mut transport = Transport::spawn(&client, &spec.cwd, &args, None, client.timeouts).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while transport.poll().unwrap().is_none() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    let (_, written) = transport.background("get_state").unwrap();

    assert!(written.await.unwrap().is_err());

    assert_eq!(transport.poll(), Err(AgentSessionError::Failed));
    assert!(
        transport
            .diagnostics
            .message()
            .unwrap()
            .contains("partial command")
    );
    assert!(matches!(
        transport.background("get_state"),
        Err(AgentSessionError::Failed)
    ));
    assert_eq!(
        transport.send(&json!({"type":"get_state"})).await,
        Err(AgentSessionError::Failed)
    );
    transport.close().await.unwrap();
}
