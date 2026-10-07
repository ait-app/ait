use super::*;

#[test]
fn mark_unread_does_not_mutate_a_candidate_changed_after_selection() {
    for moved in [false, true] {
        let (service, agents, _) = service();
        let mut initial = agent("changed", "wks-one", "2026-09-22T10:00:00.000Z");
        initial.requires_attention = false;
        agents.upsert(&initial).unwrap();
        let mut current = initial;
        if moved {
            current.workspace_id = Some("wks-two".to_owned());
        } else {
            current.updated_at = "2026-09-22T12:00:00.000Z".to_owned();
        }
        agents.2.lock().unwrap().replacement = Some(current.clone());

        assert_eq!(
            service.mark_unread("wks-one", "2026-09-22T11:00:00.000Z"),
            Err(WorkspaceStateError::AgentNoLongerFinished(
                "changed".to_owned()
            ))
        );
        assert_eq!(agents.get("changed").unwrap(), Some(current));
    }
}

#[test]
fn a_stale_attention_scan_preserves_a_new_permission_request() {
    let agents = Agents::default();
    let initial = agent("root", "wks-one", "2026-09-22T10:00:00.000Z");
    agents.upsert(&initial).unwrap();
    let attention = AgentWorkspaceAttention::new(Box::new(agents.clone()));
    let scan = attention.scan().unwrap();
    let mut current = initial;
    current.last_status = AgentRuntimeStatus::Running;
    current.attention_reason = Some(AgentAttentionReason::Permission);
    current.updated_at = "2026-09-22T12:00:00.000Z".to_owned();
    current.attention_timestamp = Some(current.updated_at.clone());
    agents.upsert(&current).unwrap();

    let result = scan.clear_attention("wks-one", "2026-09-22T11:00:00.000Z");

    assert!(result.error.is_none());
    assert!(result.cleared_agent_ids.is_empty());
    assert_eq!(agents.get("root").unwrap(), Some(current));
}

#[test]
fn a_stale_attention_scan_preserves_new_finished_attention_and_workspace_placement() {
    for moved in [false, true] {
        let agents = Agents::default();
        let initial = agent("root", "wks-one", "2026-09-22T10:00:00.000Z");
        agents.upsert(&initial).unwrap();
        let attention = AgentWorkspaceAttention::new(Box::new(agents.clone()));
        let scan = attention.scan().unwrap();
        let mut current = initial;
        if moved {
            current.workspace_id = Some("wks-two".to_owned());
        } else {
            current.updated_at = "2026-09-22T12:00:00.000Z".to_owned();
            current.attention_timestamp = Some(current.updated_at.clone());
        }
        agents.upsert(&current).unwrap();

        let result = scan.clear_attention("wks-one", "2026-09-22T11:00:00.000Z");

        assert!(result.error.is_none());
        assert!(result.cleared_agent_ids.is_empty());
        assert_eq!(agents.get("root").unwrap(), Some(current));
    }
}
