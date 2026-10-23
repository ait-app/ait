use super::*;
use crate::host::hub::tests::sources;
use domain::agent_runtime::PersistedAgentRuntimeRecord;

#[test]
fn timeline_projection_excludes_history_and_uncommitted_deltas() {
    assert!(
        Event::project(
            "agent_stream",
            &json!({"agentId":"agent","event":{"secret":"text"}})
        )
        .is_none()
    );
    let event = Event::project(
        "agent_stream",
        &json!({"agentId":"agent","epoch":"epoch","seq":7,"event":{"text":"history"}}),
    )
    .unwrap();
    assert_eq!(event.method, "agent.timeline.update");
    assert_eq!(
        event.params,
        json!({"agentId":"agent","epoch":"epoch","seq":7})
    );
    let reset = Event::project(
        "agent.timeline.replacement",
        &json!({"agentId":"agent","epoch":"new"}),
    )
    .unwrap();
    assert_eq!(reset.params["reset"], true);
    assert!(
        Event::project(
            "status.daemon_config_changed",
            &json!({"credentials":"secret"})
        )
        .is_none()
    );
    assert!(Event::project("creation.update", &json!({"phase":"done"})).is_some());
    let creation = Event::project(
        "agent.create.update",
        &json!({
            "kind":"agent","phase":"completed","agentId":"agent","revision":3,
            "agent":{"config":{"systemPrompt":"private"}}
        }),
    )
    .unwrap();
    assert_eq!(creation.method, "creation.update");
    assert_eq!(creation.params["agentId"], "agent");
    assert!(!creation.params.to_string().contains("private"));
    assert!(Event::project("creation.update", &json!({"huge":"x".repeat(64*1024)})).is_none());
}

#[test]
fn terminal_notification_is_utf8_bounded_and_does_not_upload_paths() {
    let directory = tempfile::tempdir().unwrap();
    let event = Event::project(
        "terminal_attention_required",
        &json!({
            "terminalId":"terminal","workspaceId":"workspace","cwd":"/private/path",
            "title":"完成".repeat(120),"body":"body"
        }),
    )
    .unwrap();
    let publication = event
        .publication(&sources(directory.path()), "server", true)
        .unwrap();
    assert!(event.attention());
    assert_eq!(publication["resourceId"], "terminal");
    assert!(publication["notification"]["title"].as_str().unwrap().len() <= 200);
    assert!(publication["notification"]["data"].get("cwd").is_none());
    assert!(
        publication["notification"]["data"]
            .get("serverId")
            .is_none()
    );
    assert_eq!(truncate("完成", 4), "完");
}

#[test]
fn agent_notifications_validate_durable_attention_and_retry_payload_has_no_credentials() {
    let directory = tempfile::tempdir().unwrap();
    let sources = sources(directory.path());
    let mut record: PersistedAgentRuntimeRecord = serde_json::from_value(json!({
        "id":"agent","provider":"test","cwd":"/private/path","createdAt":"now","updatedAt":"now",
        "workspaceId":"workspace","title":"Task","requiresAttention":true,
        "attentionReason":"finished","attentionTimestamp":"now",
        "config":{"systemPrompt":"secret","mcpServers":{"credentials":"secret"}}
    }))
    .unwrap();
    sources.agents.upsert(&record).unwrap();
    let event = Event::project(
        "agent_attention_required",
        &json!({"agentId":"agent","timestamp":"now","reason":"finished"}),
    )
    .unwrap();
    let publication = event.publication(&sources, "server", true).unwrap();
    assert_eq!(publication["notification"]["title"], "Agent finished");
    assert!(!publication.to_string().contains("secret"));
    assert!(Event::current_notification(&publication, &sources, "server").unwrap());
    assert!(
        event
            .publication(&sources, "server", false)
            .unwrap()
            .get("notification")
            .is_none()
    );
    record.requires_attention = false;
    sources.agents.upsert(&record).unwrap();
    assert!(!Event::current_notification(&publication, &sources, "server").unwrap());
    assert!(
        event
            .publication(&sources, "server", true)
            .unwrap()
            .get("notification")
            .is_none()
    );
    let permission =
        Event::project("agent_permission_request", &json!({"agentId":"agent"})).unwrap();
    assert!(!permission.attention());
    assert!(
        permission
            .publication(&sources, "server", true)
            .unwrap()
            .get("notification")
            .is_none()
    );
}
