use super::*;

#[test]
fn persisted_main_host_card_remains_between_consecutive_assistant_messages() {
    let items = [
        json!({"type":"assistant_message","text":"Before host result"}),
        json!({"type":"tool_call","callId":"host-setup","name":"paseo_worktree_setup",
            "status":"completed","error":null,"detail":{"type":"worktree_setup",
                "worktreePath":"/saved/worktree","branchName":"feature","log":"Saved host result",
                "commands":[{"index":0,"command":"npm ci","cwd":"/saved/worktree",
                    "log":"Complete setup output","status":"completed","exitCode":0}]}}),
        json!({"type":"assistant_message","text":"After host result"}),
    ];
    let source: Vec<_> = items
        .iter()
        .enumerate()
        .map(|(index, item)| NativeItem {
            key: format!("saved:{index}"),
            turn_id: Some("turn".to_owned()),
            timestamp: "2026-12-01T00:00:00Z".to_owned(),
            item: item.clone(),
        })
        .collect();
    let timeline = crate::storage::timeline::Timeline::memory().unwrap();
    timeline.append("agent", "codex", &source).unwrap();
    let saved = timeline
        .export_rows("agent", &CancellationToken::new())
        .unwrap();
    let projected = crate::timeline_projection::project(&saved);
    assert_eq!(projected.len(), 3);
    for (visible, original) in projected.iter().zip(&items) {
        assert_eq!(
            &visible.item, original,
            "export must retain the existing visible card/body"
        );
    }
    for format in [Format::Markdown, Format::Html] {
        let (whole, count) = content(format, Scope::Timeline, &projected);
        assert_eq!(count, 3);
        assert!(
            whole.find("Before host result").unwrap() < whole.find("Saved host result").unwrap()
        );
        assert!(
            whole.find("Saved host result").unwrap() < whole.find("After host result").unwrap()
        );
        assert!(whole.contains("Complete setup output"));
        let (conversation, count) = content(format, Scope::Conversation, &projected);
        assert_eq!(count, 2);
        assert!(!conversation.contains("Saved host result"));
        assert!(conversation.contains("Before host result"));
        assert!(conversation.contains("After host result"));
    }
}
