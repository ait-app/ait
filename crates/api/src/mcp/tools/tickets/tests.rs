use super::*;

#[test]
fn ticket_tools_are_routed_here_and_others_are_not() {
    for tool in ["bot_ticket_list", "bot_ticket_update", "ticket_report"] {
        assert!(handles(tool), "{tool}");
    }
    for tool in [
        "agent_create",
        "bot_overview",
        "bot_memory_read",
        "bot_read",
        "ticket_read",
    ] {
        assert!(!handles(tool), "{tool}");
    }
}

#[test]
fn every_join_names_its_ticket() {
    let ticket = TicketId::generate();
    let opened = OpenedTicket {
        ticket_id: ticket,
        created: false,
    };
    for joined in [
        Join::Opened(opened),
        Join::Named(ticket),
        Join::Inherited(ticket),
    ] {
        assert_eq!(joined.ticket(), ticket, "{joined:?}");
    }
}

#[test]
fn at_the_limit_unclosed_tickets_with_agents_or_past_their_grace_without_are_leftovers() {
    let now = Utc::now();
    let opened = |status: &str, agents: usize, age: chrono::TimeDelta| {
        let assignments: Vec<Value> = (0..agents)
            .map(|index| json!({"agentId":format!("agent-{index}"),"workspaceId":"wks_1"}))
            .collect();
        json!({"id":TicketId::generate(),"status":status,"assignments":assignments,
            "createdAt":now - age})
    };
    let old = ORPHAN_GRACE + chrono::TimeDelta::seconds(1);
    let young = ORPHAN_GRACE - chrono::TimeDelta::seconds(1);
    for (ticket, expected) in [
        (opened("open", 1, young), Some(Leftover::Finished)),
        (opened("waiting", 2, old), Some(Leftover::Finished)),
        (opened("open", 0, old), Some(Leftover::Orphaned)),
        (opened("waiting", 0, old), Some(Leftover::Orphaned)),
        // Its first Agent may still be on the way.
        (opened("open", 0, young), None),
        (opened("done", 1, old), None),
        (opened("cancelled", 0, old), None),
        (
            json!({"id":"not-a-ticket","status":"open","assignments":[]}),
            None,
        ),
    ] {
        let found = leftover(&ticket, now);
        assert_eq!(found.map(|(_, kind)| kind), expected, "{ticket}");
        if let Some((id, _)) = found {
            assert_eq!(json!(id), ticket["id"]);
        }
    }
}
