use std::collections::BTreeMap;

use domain::{ConversationId, TicketId};

use super::*;

fn sight(viewer: Viewer) -> Sight {
    Sight {
        viewer,
        tickets: BTreeMap::new(),
    }
}

#[test]
fn read_tools_are_routed_here_and_others_are_not() {
    for tool in ["bot_read", "ticket_read"] {
        assert!(handles(tool), "{tool}");
    }
    for tool in ["bot_tickets", "workspace_overview", "get_agent_activity"] {
        assert!(!handles(tool), "{tool}");
    }
}

#[test]
fn offsets_and_limits_are_optional_non_negative_integers() {
    assert_eq!(count(&Value::Null), Ok(None));
    assert_eq!(count(&json!(7)), Ok(Some(7)));
    assert_eq!(count(&json!(-1)), Err(ErrorCode::InvalidMessage));
    assert_eq!(count(&json!("7")), Err(ErrorCode::InvalidMessage));
}

#[test]
fn other_conversations_tickets_shrink_to_their_title_and_status() {
    let (own, other) = (ConversationId::generate(), ConversationId::generate());
    let ticket = |id: &str, conversation: Value| {
        json!({"id":id,"title":"Task","status":"waiting",
            "waiting":{"reason":"blocked","agentId":"agent-1","since":"t"},
            "conversationId":conversation,"updatedAt":"t","closedAt":null,"outcome":null,
            "assignments":[{"agentId":"agent-1"}],"latestReport":{"text":"secret"}})
    };
    let listed = json!({"tickets":[ticket("mine", json!(own)), ticket("theirs", json!(other)),
        ticket("nobody", Value::Null)]});

    let mut external = listed.clone();
    restrict_tickets(&mut external, Some(&sight(Viewer::External(own))));
    assert_eq!(external["tickets"][0], listed["tickets"][0]);
    for index in [1, 2] {
        let restricted = &external["tickets"][index];
        assert_eq!(restricted["restricted"], true);
        assert_eq!(restricted["title"], "Task");
        assert!(restricted.get("latestReport").is_none());
        assert!(restricted.get("conversationId").is_none());
        assert!(restricted.get("assignments").is_none());
        assert_eq!(
            restricted["waiting"],
            json!({"reason":"blocked","since":"t"})
        );
        assert!(!restricted.to_string().contains("agent-1"));
    }
    for viewer in [Some(sight(Viewer::Owner(own))), None] {
        let mut unchanged = listed.clone();
        restrict_tickets(&mut unchanged, viewer.as_ref());
        assert_eq!(unchanged, listed);
    }
    assert!(!sees(
        Some(&sight(Viewer::Worker(TicketId::generate()))),
        &json!(own)
    ));
    assert!(!sees(
        Some(&sight(Viewer::External(own))),
        &json!("not-a-conversation")
    ));
}

#[test]
fn only_an_external_sight_restricts_agents() {
    let own = ConversationId::generate();
    let unlabelled = json!({"id":"agent-1","labels":{}});
    let mine = json!({"id":"agent-2","labels":{domain::bot::BOT_CONVERSATION_LABEL:own}});
    let stranger = sight(Viewer::External(own));
    assert!(external(Some(&stranger)));
    assert!(!sees_agent(Some(&stranger), &unlabelled));
    assert!(sees_agent(Some(&stranger), &mine));
    for open in [Some(sight(Viewer::Owner(own))), None] {
        assert!(!external(open.as_ref()));
        assert!(sees_agent(open.as_ref(), &unlabelled));
    }
}
