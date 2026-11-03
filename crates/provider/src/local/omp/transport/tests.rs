use super::*;
use crate::ports::agent_session::AgentClient;

#[tokio::test]
async fn slow_native_startup_preserves_the_connected_control_deadline() {
    let (root, mut client, spec) = crate::local::omp::tests::fixture();
    std::fs::create_dir_all(root.path().join("agent")).unwrap();
    std::fs::write(root.path().join("agent/ready-delay"), "0.8").unwrap();
    std::fs::write(root.path().join("agent/negotiate-delay"), "0.8").unwrap();
    client.deadline = Duration::from_millis(200);
    assert!(client.discovery_timeout() > STARTUP_TIMEOUT);
    let mut transport = Transport::spawn(&client, &spec.cwd, &[]).unwrap();
    transport.initialize().await.unwrap();
    assert_eq!(
        transport
            .request("fixture_slow_control", json!({}))
            .await
            .unwrap_err(),
        AgentSessionError::Failed
    );
    transport.close().await.unwrap();
}
