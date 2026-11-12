use super::*;

#[test]
fn ticket_tools_are_routed_here_and_others_are_not() {
    for tool in ["bot_tickets", "bot_ticket_update", "report_progress"] {
        assert!(handles(tool), "{tool}");
    }
    for tool in ["create_agent", "workspace_overview", "bot_memory_read"] {
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
