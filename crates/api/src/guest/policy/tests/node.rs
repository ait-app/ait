//! Whole-node administration is separate from narrower resource grants.
use super::*;

#[tokio::test]
async fn management_requires_a_whole_node_grant() {
    for kind in [
        ResourceKind::Workspace,
        ResourceKind::Project,
        ResourceKind::Node,
    ] {
        let guest = Guest::with(kind).await;
        for method in [
            "daemon.get_status.request",
            "daemon.config.get.request",
            "daemon.config.set.request",
            "daemon.config.reload.request",
            "daemon.update.request",
            "server.restart.request",
            "server.shutdown.request",
            "diagnostics.request",
            "schedule.create.request",
            "bot.list.request",
            "provider.usage.list.request",
            "agent.skills.get_status.request",
            "session.events.set_subscription.request",
            "file.upload.request",
        ] {
            if kind == ResourceKind::Node {
                assert_eq!(
                    guest.forwarded(method, json!({})).await,
                    json!({}),
                    "{method}"
                );
            } else {
                assert_eq!(
                    guest.refused(method, json!({})).await,
                    ErrorCode::Forbidden,
                    "{method}"
                );
            }
        }
        for method in [
            "share.list.request",
            "share.set.request",
            "share.remove.request",
            "publication.authorize.request",
            "publication.stop.request",
            "relay.start.request",
            "unknown.method",
            "daemon.unknown.request",
        ] {
            assert_eq!(
                guest.refused(method, json!({})).await,
                ErrorCode::Forbidden,
                "{method}"
            );
        }
    }
}

#[tokio::test]
async fn node_visitors_drive_and_resize_other_peoples_terminals() {
    let guest = Guest::with(ResourceKind::Node).await;
    for id in ["owner-terminal", "other-visitor-terminal"] {
        for method in ["terminal.kill.request", "terminal.rename.request"] {
            let params = json!({"terminalId": id, "title": "renamed"});
            assert_eq!(guest.forwarded(method, params.clone()).await, params);
        }
        assert_eq!(
            event(&guest.session, "terminal.input", &json!({"terminalId": id})),
            EventDecision::Forward
        );
        // Both binary input and resize use this guard in the connection dispatcher.
        assert!(terminals::owned(&guest.session, id).is_ok());
        let params = json!({"terminalId": id, "restore": {"size": {"cols": 100, "rows": 30}}});
        assert_eq!(
            guest
                .forwarded("terminal.subscribe.request", params.clone())
                .await,
            params
        );
    }
    guest
        .forwarded(
            "agent.items.close.request",
            json!({"terminalIds": ["owner-terminal"]}),
        )
        .await;
    assert_eq!(
        event(&guest.session, "push.register", &json!({})),
        EventDecision::Refuse(ErrorCode::Forbidden)
    );
    assert_eq!(
        event(&guest.session, "session.heartbeat", &json!({})),
        EventDecision::Drop
    );
    assert_eq!(
        event(&guest.session, "unknown.event", &json!({})),
        EventDecision::Refuse(ErrorCode::Forbidden)
    );
}

#[test]
fn node_method_directions_and_authority_are_checked() {
    use model::methods::InboundKind;
    assert!(super::super::node::allows(
        "server.restart.request",
        InboundKind::Request
    ));
    assert!(!super::super::node::allows(
        "server.restart.request",
        InboundKind::Event
    ));
    assert!(!super::super::node::allows(
        "share.set.request",
        InboundKind::Request
    ));
    assert!(!super::super::node::allows(
        "hub.shares.publish",
        InboundKind::Request
    ));
    assert!(!super::super::node::allows("unknown", InboundKind::Request));
}
