use std::collections::BTreeMap;

use crate::protocol::agent_lifecycle::{AgentHistoryRequest, AgentListRequest};
use domain::agent_runtime::{AgentRuntimeStatus, PersistedAgentRuntimeRecord};
use serde_json::json;

use super::{QueryScope, project, query, snapshot};
use crate::service::agent_runtime::{AgentPlacement, ThinkingOptionFilter};

#[test]
fn history_defaults_to_archived_while_active_list_does_not() {
    let list: AgentListRequest = serde_json::from_value(json!({})).expect("list request");
    let history: AgentHistoryRequest = serde_json::from_value(json!({})).expect("history request");

    let list_query =
        query(list.filter, list.sort, list.page, QueryScope::All, None).expect("list query");
    let history_query = query(
        history.filter,
        history.sort,
        history.page,
        QueryScope::History,
        history.search,
    )
    .expect("history query");

    assert!(!list_query.include_archived);
    assert!(history_query.include_archived);
    assert_eq!(list_query.limit, 200);

    let empty_filters: AgentListRequest = serde_json::from_value(json!({
        "filter": {"projectKeys":["  "],"statuses":[]}
    }))
    .expect("empty filters");
    let empty_query = query(
        empty_filters.filter,
        empty_filters.sort,
        empty_filters.page,
        QueryScope::All,
        None,
    )
    .expect("empty filters should be no-ops");
    assert!(empty_query.project_keys.is_none());
    assert!(empty_query.statuses.is_none());
}

#[test]
fn thinking_filter_mapping_preserves_missing_null_and_selected_values() {
    for (filter, expected) in [
        (json!({}), None),
        (
            json!({"thinkingOptionId": null}),
            Some(ThinkingOptionFilter::ProviderDefault),
        ),
        (
            json!({"thinkingOptionId": "high"}),
            Some(ThinkingOptionFilter::Selected("high".to_owned())),
        ),
    ] {
        let request: AgentListRequest =
            serde_json::from_value(json!({"filter": filter})).expect("filter request");
        let actual = query(
            request.filter,
            request.sort,
            request.page,
            QueryScope::All,
            None,
        )
        .expect("directory query");
        assert_eq!(actual.thinking_option_id, expected);
    }
}

#[test]
fn query_preserves_status_filters_sort_precedence_and_page_boundaries() {
    use crate::service::agent_runtime::{AgentSortKey, SortDirection};
    let request: AgentListRequest = serde_json::from_value(json!({
        "filter": {"statuses":["initializing","idle","running","error","closed","idle"], "includeArchived":true},
        "sort": [
            {"key":"status_priority","direction":"desc"},
            {"key":"created_at","direction":"asc"},
            {"key":"updated_at","direction":"desc"},
            {"key":"title","direction":"asc"}
        ],
        "page":{"limit":1,"cursor":"continuation"}
    })).unwrap();
    let query = query(
        request.filter,
        request.sort,
        request.page,
        QueryScope::Active,
        Some("needle".to_owned()),
    )
    .unwrap();
    assert!(query.active_scope && query.include_archived);
    assert_eq!(
        query.statuses.unwrap().into_iter().collect::<Vec<_>>(),
        [
            AgentRuntimeStatus::Initializing,
            AgentRuntimeStatus::Idle,
            AgentRuntimeStatus::Running,
            AgentRuntimeStatus::Error,
            AgentRuntimeStatus::Closed
        ]
    );
    assert_eq!(
        query
            .sort
            .iter()
            .map(|sort| (sort.key, sort.direction))
            .collect::<Vec<_>>(),
        [
            (AgentSortKey::StatusPriority, SortDirection::Desc),
            (AgentSortKey::CreatedAt, SortDirection::Asc),
            (AgentSortKey::UpdatedAt, SortDirection::Desc),
            (AgentSortKey::Title, SortDirection::Asc)
        ]
    );
    assert_eq!(query.limit, 1);
    assert_eq!(query.cursor.as_deref(), Some("continuation"));
    assert_eq!(query.search.as_deref(), Some("needle"));
}

#[test]
fn invalid_page_sizes_are_rejected_before_querying_the_registry() {
    for limit in [0, 201, usize::MAX] {
        let request: AgentListRequest =
            serde_json::from_value(json!({"page":{"limit":limit}})).unwrap();
        assert!(matches!(
            query(
                request.filter,
                request.sort,
                request.page,
                QueryScope::All,
                None
            ),
            Err(super::ErrorCode::InvalidMessage)
        ));
    }
}

#[test]
fn stored_projection_marks_provider_unavailable_and_hides_resume_handle() {
    let mut record = record();
    record.updated_at = "2026-09-20T09:00:00-02:00".to_owned();
    record.last_activity_at = Some("2026-09-20T12:00:00+00:00".to_owned());
    record.features = vec![json!({"id":"stored-only"})];
    record.last_error = Some("old provider error".to_owned());
    record.persistence = Some(domain::agent_runtime::AgentPersistenceHandle {
        provider: "codex".to_owned(),
        session_id: "thread-1".to_owned(),
        native_handle: None,
        metadata: None,
    });

    let projected = snapshot(&record);
    assert!(projected.provider_unavailable);
    assert!(projected.persistence.is_none());
    assert_eq!(
        projected.capabilities.get("supportsSessionPersistence"),
        Some(&true)
    );
    assert_eq!(
        projected.status,
        crate::protocol::agent_lifecycle::AgentStatus::Closed
    );
    assert_eq!(projected.updated_at, "2026-09-20T12:00:00.000Z");
    assert!(projected.features.is_empty());
    assert!(projected.last_error.is_none());
}

#[test]
fn placement_projection_preserves_managed_worktree_identity() {
    let projected = project(&AgentPlacement {
        active: true,
        project_key: "github:owner/repo".to_owned(),
        project_name: "Repo".to_owned(),
        workspace_name: "Feature".to_owned(),
        cwd: "/worktrees/feature/subdir".to_owned(),
        is_git: true,
        current_branch: Some("feature".to_owned()),
        worktree_root: Some("/worktrees/feature".to_owned()),
        is_paseo_owned_worktree: true,
        main_repo_root: Some("/repo".to_owned()),
    });

    assert_eq!(projected.workspace_name, Some("Feature".to_owned()));
    assert!(projected.checkout.is_paseo_owned_worktree);
    assert_eq!(projected.checkout.main_repo_root.as_deref(), Some("/repo"));
}

fn record() -> PersistedAgentRuntimeRecord {
    PersistedAgentRuntimeRecord {
        id: "agent-1".to_owned(),
        provider: "codex".to_owned(),
        cwd: "/repo".to_owned(),
        workspace_id: Some("wks-1".to_owned()),
        created_at: "2026-09-20T00:00:00.000Z".to_owned(),
        updated_at: "2026-09-20T00:01:00.000Z".to_owned(),
        last_activity_at: None,
        last_user_message_at: None,
        title: Some("Agent".to_owned()),
        title_origin: None,
        labels: BTreeMap::new(),
        last_status: AgentRuntimeStatus::Closed,
        last_mode_id: None,
        config: None,
        runtime_info: None,
        features: Vec::new(),
        persistence: None,
        last_error: None,
        requires_attention: false,
        attention_reason: None,
        attention_timestamp: None,
        internal: false,
        archived_at: None,
        owner: None,
    }
}
