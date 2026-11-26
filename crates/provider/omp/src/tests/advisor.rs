use super::*;

#[tokio::test]
async fn advisor_shortcut_uses_latest_host_configuration_without_sending_a_prompt() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    commands_ready(session.as_mut()).await;
    let before = calls_after(root.path(), 0);
    let mut config = spec.config.clone();
    for (text, expected) in [
        ("/advisor on", true),
        ("/advisor toggle", false),
        ("/advisor", true),
        ("/advisor off", false),
    ] {
        let patch = session
            .command_config_patch(&AgentPrompt::text(text), &config)
            .unwrap()
            .unwrap();
        assert_eq!(
            patch.feature_values.as_ref().unwrap()["advisorEnabled"],
            expected
        );
        config.feature_values = patch.feature_values;
    }
    assert!(
        session
            .command_config_patch(&AgentPrompt::text("/advisor invalid"), &config)
            .is_err()
    );
    assert_eq!(calls_after(root.path(), 0), before);
    session.close().await.unwrap();
}
