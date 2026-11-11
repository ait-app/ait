use std::io::Read;

use super::*;
use crate::ports::timeline_export::{Format, Request, Scope};

#[tokio::test]
async fn saved_export_during_active_turn_never_inputs_restores_or_interrupts_native_session() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"hang"}),
        )
        .await
        .unwrap();
    let before = registry.get(id).unwrap().unwrap();
    let requests = fixture.requests();
    let rows = execution
        .timeline()
        .export_rows(id, &CancellationToken::new())
        .unwrap();
    for format in [Format::Markdown, Format::Html] {
        let mut export = tokio::time::timeout(
            Duration::from_secs(2),
            execution.export_timeline(Request {
                agent_id: id.to_owned(),
                format,
                scope: Scope::Timeline,
            }),
        )
        .await
        .expect("export must not queue behind the active native turn")
        .unwrap();
        let mut downloaded = String::new();
        export
            .artifact
            .file
            .read_to_string(&mut downloaded)
            .unwrap();
        assert!(downloaded.contains("Ait saved Timeline"));
        assert_eq!(export.artifact.agent_id, id);
    }
    assert_eq!(
        fixture.requests(),
        requests,
        "export issued native input or restoration"
    );
    assert_eq!(
        registry.get(id).unwrap().unwrap(),
        before,
        "export mutated runtime state"
    );
    assert_eq!(
        execution
            .timeline()
            .export_rows(id, &CancellationToken::new())
            .unwrap(),
        rows
    );
    assert_eq!(
        execution
            .execute(
                "agent.finish.wait.request",
                json!({"agentId":id,"timeoutMs":1})
            )
            .await
            .unwrap()["status"],
        "timeout"
    );
    execution
        .execute("agent.cancel.request", json!({"agentId":id}))
        .await
        .unwrap();
    execution
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn export_admission_is_bounded_and_missing_agents_do_not_restore_any_provider() {
    let fixture = Fixture::new();
    let (execution, _) = worker(&fixture);
    let permits = Arc::clone(&execution.0.exports)
        .try_acquire_many_owned(2)
        .unwrap();
    assert!(matches!(
        execution
            .export_timeline(Request {
                agent_id: "missing".to_owned(),
                format: Format::Markdown,
                scope: Scope::Timeline,
            })
            .await,
        Err(model::ErrorCode::ResourceExhausted)
    ));
    drop(permits);
    assert!(matches!(
        execution
            .export_timeline(Request {
                agent_id: "missing".to_owned(),
                format: Format::Html,
                scope: Scope::Timeline,
            })
            .await,
        Err(model::ErrorCode::AgentNotFound)
    ));
    assert!(!fixture.cwd.join("native-requests.jsonl").exists());
    execution.shutdown().await.unwrap();
}
