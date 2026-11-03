use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

use super::*;

#[test]
fn filtering_and_pagination_borrow_committed_records_and_observe_the_next_commit() {
    let (mut directory, agents) = service();
    let root = tempfile::tempdir().unwrap();
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    for record in agents.list().unwrap() {
        registry.upsert(&record).unwrap();
    }
    registry
        .upsert(&agent("internal", "wks-one", "Hidden", true))
        .unwrap();
    directory.agents = Arc::new(registry.clone());
    let committed = registry.snapshot().unwrap();
    let query = AgentDirectoryQuery {
        limit: 1,
        ..AgentDirectoryQuery::default()
    };
    let matching = directory.matching_entries(&query).unwrap();
    assert_eq!(matching.len(), 2);
    for entry in &matching {
        let original = committed
            .iter()
            .find(|record| record.id == entry.agent.id)
            .unwrap();
        assert!(Arc::ptr_eq(original, &entry.agent));
    }
    let first = directory.list(&query).unwrap();
    assert!(first.has_more);
    let previous = first.entries[0].agent.clone();
    registry
        .update(&previous.id, &|current| {
            let mut next = current.clone();
            next.title = Some("Updated".to_owned());
            next.requires_attention = true;
            next.attention_reason = Some(AgentAttentionReason::Permission);
            next
        })
        .unwrap();
    let current = directory.get(&previous.id).unwrap();
    assert_eq!(current.agent.title.as_deref(), Some("Updated"));
    assert!(current.agent.requires_attention);
    assert_eq!(
        current.agent.attention_reason,
        Some(AgentAttentionReason::Permission)
    );
    assert_ne!(previous.title.as_deref(), Some("Updated"));
    let next = directory
        .list(&AgentDirectoryQuery {
            cursor: first.next_cursor,
            ..query
        })
        .unwrap();
    assert_eq!(next.entries.len(), 1);
    assert_ne!(next.entries[0].agent.id, previous.id);
    registry.remove(&next.entries[0].agent.id).unwrap();
    assert_eq!(
        directory
            .matching_entries(&AgentDirectoryQuery::default())
            .unwrap()
            .len(),
        1
    );
}
