use super::super::client::OpenCodeClient;
use super::super::tests::fixture::Fixture;
use crate::local::opencode::protocol::Version;
use crate::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSessionSpec, AgentTurnEvent,
};
use domain::agent_runtime::StoredAgentConfig;
use serde_json::json;

#[tokio::test]
async fn native_policies_survive_creation_restore_and_agent_switching() {
    for version in [Version::V1, Version::V2] {
        let fixture = Fixture::start(version).await;
        let policy = match version {
            Version::V1 => json!([{"permission":"bash","pattern":"git push *","action":"deny"}]),
            Version::V2 => json!([{"action":"shell","resource":"git push *","effect":"deny"}]),
        };
        fixture.state.lock().unwrap().permission = policy.clone();
        let client = OpenCodeClient::new(fixture.binary.clone());
        let mut spec = AgentSessionSpec {
            provider: "opencode".into(),
            cwd: fixture.cwd.to_str().unwrap().into(),
            config: StoredAgentConfig {
                model: Some("local/test-model".into()),
                mode_id: Some("build".into()),
                ..Default::default()
            },
        };
        let mut session = client.create_session(&spec).await.unwrap();
        spec.config.mode_id = Some("plan".into());
        session.start_turn("plan", &spec.config).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if matches!(
                    session.poll_turn().unwrap(),
                    Some(AgentTurnEvent::Completed(_))
                ) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            session.runtime_info().await.unwrap().mode_id.as_deref(),
            Some("plan")
        );
        assert_eq!(fixture.state.lock().unwrap().agent, "plan");
        let handle = session.persistence().unwrap();
        session.close().await.unwrap();
        spec.config.mode_id = Some("build".into());
        let mut restored = client
            .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
            .await
            .unwrap();
        assert_eq!(
            restored.runtime_info().await.unwrap().mode_id.as_deref(),
            Some("build")
        );
        restored.close().await.unwrap();
        let state = fixture.state.lock().unwrap();
        assert_eq!(state.permission, policy);
        assert_eq!(state.permission_updates, 0);
    }
}
