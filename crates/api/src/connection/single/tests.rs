use super::*;

#[test]
fn worker_lanes_keep_streams_and_subscriptions_with_their_connection_owner() {
    for (method, expected) in [
        ("terminal.input", 1),
        ("terminal.list.request", 1),
        ("voice.abort.request", 1),
        ("dictation.stream.start", 1),
        ("checkout.diff.subscribe.request", 2),
        ("file.upload.request", 2),
        ("agent.skills.get_status.request", 2),
        ("agent.skills.reconcile.request", 2),
        ("agent.skills.uninstall.request", 2),
        ("agent.skills.save_selection.request", 2),
        ("agent.skills.import_legacy_selection.request", 2),
        ("workspace.worktree.list.request", 2),
        ("workspace.worktree.create.request", 2),
        ("workspace.worktree.archive.request", 2),
        ("workspace.recovery.inspect.request", 2),
        ("workspace.recovery.restore.request", 2),
        ("workspace.github.search_repositories.request", 2),
        ("project.github.clone.request", 2),
        ("agent.list.request", 3),
        ("agent.timeline.set_subscription.request", 3),
        ("workspace.create.request", 0),
        ("workspace.archive.request", 0),
        ("project.add.request", 0),
        ("schedule.list.request", 0),
        ("browser.host.register.request", 0),
        ("relay.status.request", 0),
        ("unknown.request", 0),
        ("agent.skills.unknown.request", 0),
        ("workspace.worktree.unknown.request", 0),
        ("project.github.unknown.request", 0),
    ] {
        let message = Incoming::Text(
            ClientMessage::Request {
                request_id: "r1".to_owned(),
                method: method.to_owned(),
                params: serde_json::json!({}),
            },
            None,
        );
        assert_eq!(lane(&message), expected, "{method}");
    }
    assert_eq!(lane(&Incoming::Binary(vec![0x01])), 1);
    assert_eq!(lane(&Incoming::Binary(vec![0x10])), 2);
    assert_eq!(lane(&Incoming::Binary(Vec::new())), 2);
}
