use serde_json::json;

use crate::protocol::agent_config::NullableSetting;

use super::{AgentAttentionClearRequest, AgentHistoryRequest, AgentIdSelection, AgentListRequest};

#[test]
fn decodes_paseo_directory_filters_and_explicit_null_thinking() {
    let request: AgentListRequest = serde_json::from_value(json!({
        "scope": "active",
        "filter": {
            "labels": {"team": "server"},
            "statuses": ["idle", "closed"],
            "thinkingOptionId": null
        },
        "sort": [{"key": "updated_at", "direction": "desc"}],
        "page": {"limit": 20, "cursor": "20"}
    }))
    .expect("request should decode");

    assert_eq!(
        request
            .filter
            .expect("filter should exist")
            .thinking_option_id,
        NullableSetting::Clear
    );
}

#[test]
fn history_omits_thinking_filter_when_absent() {
    let request: AgentHistoryRequest =
        serde_json::from_value(json!({})).expect("empty history request should decode");
    assert_eq!(request.filter.map(|filter| filter.thinking_option_id), None);
    let request: AgentHistoryRequest =
        serde_json::from_value(json!({"filter": {}})).expect("empty filter should decode");
    assert_eq!(
        request.filter.expect("filter").thinking_option_id,
        NullableSetting::Unchanged
    );
}

#[test]
fn selected_thinking_filter_keeps_its_identifier() {
    let request: AgentListRequest = serde_json::from_value(json!({
        "filter": {"thinkingOptionId": "high"}
    }))
    .expect("selected thinking filter");
    assert_eq!(
        request.filter.expect("filter").thinking_option_id,
        NullableSetting::Set("high".to_owned())
    );
}

#[test]
fn clear_attention_accepts_one_or_many_agent_ids() {
    let one: AgentAttentionClearRequest =
        serde_json::from_value(json!({"agentId": "a-1"})).expect("single identity should decode");
    let many: AgentAttentionClearRequest =
        serde_json::from_value(json!({"agentId": ["a-1", "a-2"]}))
            .expect("identity array should decode");

    assert_eq!(one.agent_id, AgentIdSelection::One("a-1".to_owned()));
    assert_eq!(
        many.agent_id,
        AgentIdSelection::Many(vec!["a-1".to_owned(), "a-2".to_owned()])
    );
}

#[test]
fn capabilities_use_only_canonical_agent_runtime_names() {
    assert_eq!(
        crate::rpc::agent_runtime::METHODS
            .iter()
            .map(|method| method.name)
            .collect::<Vec<_>>(),
        [
            "agent.list.request",
            "agent.history.get.request",
            "agent.get.request",
            "agent.update.request",
            "agent.archive.request",
            "agent.delete.request",
            "agent.detach.request",
            "agent.attention.clear.request",
            "agent.items.close.request",
        ]
    );
    assert!(
        !crate::rpc::agent_runtime::METHODS
            .iter()
            .any(|method| method.name.contains('_'))
    );
}
