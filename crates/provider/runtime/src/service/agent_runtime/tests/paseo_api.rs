//! Agent directory cases adapted from Paseo session.workspaces and pagination tests.

use serde_json::json;

use super::*;
use crate::rpc::agent_runtime::execute;

#[test]
fn directory_only_rpc_rejects_unsupported_operations_before_mutating_agents() {
    let (mut service, agents) = service();
    let before = agents.list().unwrap();
    for (method, params, error) in [
        (
            "agent.list.request",
            json!({"scope":"archived"}),
            crate::rpc::ErrorCode::InvalidMessage,
        ),
        (
            "agent.list.request",
            json!({"subscribe":{}}),
            crate::rpc::ErrorCode::UnsupportedCapability,
        ),
        (
            "agent.items.close.request",
            json!({"agentIds":["agent-a"],"terminalIds":["terminal-a"]}),
            crate::rpc::ErrorCode::UnsupportedCapability,
        ),
    ] {
        assert_eq!(execute(&mut service, method, params), Err(error));
        assert_eq!(agents.list().unwrap(), before);
    }
    let active = execute(
        &mut service,
        "agent.list.request",
        json!({"scope":"active"}),
    )
    .unwrap();
    assert_eq!(active["entries"].as_array().unwrap().len(), 2);
}

#[test]
fn mutation_rpc_reports_missing_targets_and_validates_bulk_attention_before_mutating() {
    let (mut service, agents) = service();
    for method in ["agent.update.request", "agent.detach.request"] {
        let result = execute(
            &mut service,
            method,
            json!({"agentId":"missing","name":"renamed"}),
        )
        .unwrap();
        assert_eq!(result["accepted"], false);
        assert!(result["error"].is_string());
    }
    assert_eq!(
        execute(
            &mut service,
            "agent.archive.request",
            json!({"agentId":"missing"})
        ),
        Err(crate::rpc::ErrorCode::AgentNotFound)
    );
    assert_eq!(
        execute(
            &mut service,
            "agent.attention.clear.request",
            json!({"agentId":[]})
        ),
        Err(crate::rpc::ErrorCode::InvalidMessage)
    );
    assert!(
        execute(
            &mut service,
            "agent.attention.clear.request",
            json!({"agentId":["agent-a","agent-b"]})
        )
        .is_ok()
    );
    assert!(
        agents
            .list()
            .unwrap()
            .iter()
            .all(|agent| !agent.requires_attention)
    );
    assert_eq!(
        execute(&mut service, "unknown", json!({})),
        Err(crate::rpc::ErrorCode::MethodNotFound)
    );
}

#[test]
fn status_priority_sort_keeps_permission_and_error_attention_ahead_of_running_agents() {
    let (mut service, agents) = service();
    agents.0.lock().unwrap().clear();
    for (id, status, attention) in [
        (
            "permission",
            AgentRuntimeStatus::Idle,
            Some(AgentAttentionReason::Permission),
        ),
        ("error", AgentRuntimeStatus::Error, None),
        ("running", AgentRuntimeStatus::Running, None),
        ("initializing", AgentRuntimeStatus::Initializing, None),
        ("idle", AgentRuntimeStatus::Idle, None),
        ("closed", AgentRuntimeStatus::Closed, None),
    ] {
        let mut record = agent(id, "wks-one", id, false);
        record.last_status = status;
        record.attention_reason = attention;
        agents.upsert(&record).unwrap();
    }
    let result = execute(&mut service, "agent.list.request", json!({"sort":[{"key":"status_priority","direction":"asc"},{"key":"created_at","direction":"desc"}]})).unwrap();
    let ids: Vec<_> = result["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["agent"]["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "permission",
            "error",
            "running",
            "initializing",
            "closed",
            "idle"
        ]
    );
    assert_eq!(result["entries"][3]["agent"]["status"], "initializing");
}

#[test]
fn filters_exclude_nonmatching_agents_and_empty_identifiers_never_resolve() {
    let (mut service, _) = service();
    for filter in [
        json!({"labels":{"team":"missing"}}),
        json!({"projectKeys":["missing"]}),
        json!({"statuses":["running"]}),
        json!({"requiresAttention":true}),
        json!({"thinkingOptionId":"nondefault"}),
    ] {
        let result = execute(&mut service, "agent.list.request", json!({"filter":filter})).unwrap();
        assert!(
            result["entries"].as_array().unwrap().is_empty(),
            "{filter}: {result}"
        );
    }
    assert!(matches!(
        service.get(" "),
        Err(AgentRuntimeError::NotFound(_))
    ));
}

#[test]
fn archive_before_the_history_cursor_does_not_skip_an_active_agent() {
    let (mut service, agents) = service();
    let first = execute(
        &mut service,
        "agent.list.request",
        json!({"page":{"limit":1}}),
    )
    .unwrap();
    assert_eq!(first["entries"][0]["agent"]["id"], "agent-b");
    agents.remove("agent-b").unwrap();
    let cursor = &first["pageInfo"]["nextCursor"];
    let second = execute(
        &mut service,
        "agent.list.request",
        json!({"page":{"limit":1,"cursor":cursor}}),
    )
    .unwrap();
    assert_eq!(second["entries"][0]["agent"]["id"], "agent-a");
    assert_eq!(second["pageInfo"]["prevCursor"], *cursor);
}

#[test]
fn agent_cursor_is_bound_to_the_requested_sort_order() {
    let (mut service, _) = service();
    let first = execute(
        &mut service,
        "agent.history.get.request",
        json!({"page":{"limit":1}}),
    )
    .unwrap();
    assert!(
        execute(
            &mut service,
            "agent.history.get.request",
            json!({
                "sort":[{"key":"title","direction":"asc"}],
                "page":{"limit":1,"cursor":first["pageInfo"]["nextCursor"]}
            })
        )
        .is_err()
    );
}

#[test]
fn equivalent_timestamp_offsets_break_ties_by_identity() {
    let (service, agents) = service();
    for (id, time) in [
        ("agent-a", "2026-09-20T11:00:00.000Z"),
        ("agent-b", "2026-09-20T12:00:00.000+01:00"),
    ] {
        agents
            .update(id, &|current| {
                let mut record = current.clone();
                record.updated_at = time.to_owned();
                record
            })
            .unwrap();
    }
    let page = service.list(&default_query()).unwrap();
    assert_eq!(
        page.entries
            .iter()
            .map(|row| row.agent.id.as_str())
            .collect::<Vec<_>>(),
        ["agent-a", "agent-b"]
    );
}

#[test]
fn history_rpc_matches_typos_and_tokens_across_agent_and_workspace_names() {
    let (mut service, agents) = service();
    agents
        .update("agent-a", &|current| {
            let mut agent = current.clone();
            agent.title = Some("Fix terminal billing".to_owned());
            agent
        })
        .unwrap();
    for search in ["trmnl", "bulling", "Fix billing", "terminal"] {
        let result = execute(
            &mut service,
            "agent.history.get.request",
            json!({"search":search}),
        )
        .unwrap();
        assert_eq!(
            result["entries"].as_array().unwrap().len(),
            1,
            "{search}: {result}"
        );
        assert_eq!(result["entries"][0]["agent"]["id"], "agent-a");
    }
}
