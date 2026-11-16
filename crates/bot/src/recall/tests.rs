use std::collections::BTreeMap;

use domain::bot::{BOT_CONVERSATION_LABEL, BOT_TICKET_LABEL};
use domain::{ConversationId, TicketId};
use model::ErrorCode;
use serde_json::json;

use super::{Page, Read, Reader, Sight, Viewer, spill};

fn read(offset: Option<usize>, limit: Option<usize>) -> Read {
    Read {
        root: "root".into(),
        reader: Reader::Worker(TicketId::generate()),
        resource: "ticket".into(),
        offset,
        limit,
        agent: None,
    }
}

#[test]
fn short_text_stays_inline_and_long_text_spills_with_its_length() {
    assert_eq!(spill("short", 5, || unreachable!()), json!("short"));
    // Multibyte characters count once and are never split.
    let spilled = spill("héllo wörld", 4, || "turn/1/input".into());
    assert_eq!(
        spilled,
        json!({"preview":"héll","chars":11,"read":"turn/1/input"})
    );
}

#[test]
fn text_pages_walk_the_whole_text_by_characters() {
    let text = "αβγδε";
    let page = Page::text(&read(None, Some(2))).unwrap();
    assert_eq!(
        page.of_text("ticket/request", text),
        json!({"resource":"ticket/request","text":"αβ","offset":0,"chars":5,"nextOffset":2})
    );
    let last = Page::text(&read(Some(4), Some(2))).unwrap();
    assert_eq!(last.of_text("r", text)["text"], "ε");
    assert!(last.of_text("r", text)["nextOffset"].is_null());
    let beyond = Page::text(&read(Some(9), None)).unwrap();
    assert_eq!(beyond.of_text("r", text)["text"], "");
    assert!(beyond.of_text("r", text)["nextOffset"].is_null());
}

#[test]
fn list_pages_report_the_next_offset_until_the_end() {
    let items: Vec<u32> = (0..5).collect();
    let first = Page::list(&read(None, Some(2))).unwrap();
    assert_eq!(
        first.items(&items, |item| json!(item)),
        json!({"items":[0,1],"nextOffset":2})
    );
    let last = Page::list(&read(Some(4), Some(2))).unwrap();
    assert_eq!(
        last.items(&items, |item| json!(item)),
        json!({"items":[4],"nextOffset":null})
    );
    let default = Page::list(&read(None, None)).unwrap();
    assert_eq!(default.limit, super::LIST_PAGE);
}

#[test]
fn page_limits_are_bounded() {
    assert_eq!(
        Page::list(&read(None, Some(0))),
        Err(ErrorCode::InvalidMessage)
    );
    assert_eq!(
        Page::list(&read(None, Some(super::LIST_PAGE_LIMIT + 1))),
        Err(ErrorCode::InvalidMessage)
    );
    assert!(Page::text(&read(None, Some(super::TEXT_PAGE_LIMIT))).is_ok());
    assert_eq!(
        Page::text(&read(None, Some(super::TEXT_PAGE_LIMIT + 1))),
        Err(ErrorCode::InvalidMessage)
    );
}

#[test]
fn the_owner_sees_everything_and_others_only_their_own() {
    let (owner, other) = (ConversationId::generate(), ConversationId::generate());
    assert!(Viewer::Owner(owner).sees(Some(other)));
    assert!(Viewer::Owner(owner).sees(None));
    assert!(Viewer::External(other).sees(Some(other)));
    assert!(!Viewer::External(other).sees(Some(owner)));
    assert!(!Viewer::External(other).sees(None));
    assert!(!Viewer::Worker(TicketId::generate()).sees(Some(owner)));
    assert_eq!(Viewer::External(other).conversation(), Some(other));
    assert_eq!(Viewer::Worker(TicketId::generate()).conversation(), None);
}

#[test]
fn an_external_viewer_reaches_only_the_agents_of_its_own_tasks() {
    let (bob, alice, owner) = (
        ConversationId::generate(),
        ConversationId::generate(),
        ConversationId::generate(),
    );
    let (bobs, alices, unconversed, forgotten) = (
        TicketId::generate(),
        TicketId::generate(),
        TicketId::generate(),
        TicketId::generate(),
    );
    let sight = Sight {
        viewer: Viewer::External(bob),
        tickets: BTreeMap::from([
            (bobs, Some(bob)),
            (alices, Some(alice)),
            (unconversed, None),
        ]),
    };
    let agent = |labels: serde_json::Value| json!({"id":"a","labels":labels});

    // An Agent on Bob's ticket is his, whoever started it, such as the owner joining it.
    assert!(sight.sees_agent(&agent(
        json!({BOT_TICKET_LABEL:bobs,BOT_CONVERSATION_LABEL:owner})
    )));
    // Alice's ticket decides over a label naming Bob.
    assert!(!sight.sees_agent(&agent(
        json!({BOT_TICKET_LABEL:alices,BOT_CONVERSATION_LABEL:bob})
    )));
    // Without a ticket of a conversation, the Agent's own conversation decides.
    for ticket in [unconversed, forgotten] {
        assert!(sight.sees_agent(&agent(
            json!({BOT_TICKET_LABEL:ticket,BOT_CONVERSATION_LABEL:bob})
        )));
        assert!(!sight.sees_agent(&agent(
            json!({BOT_TICKET_LABEL:ticket,BOT_CONVERSATION_LABEL:alice})
        )));
    }
    assert!(sight.sees_agent(&agent(json!({BOT_CONVERSATION_LABEL:bob}))));
    // Agents the Bot did not start, and malformed labels, are nobody's.
    for labels in [
        json!({}),
        json!(null),
        json!({BOT_CONVERSATION_LABEL:"bob"}),
    ] {
        assert!(!sight.sees_agent(&agent(labels)));
    }
    // The owner reaches every Agent; a worker none through this rule.
    let all = agent(json!({}));
    assert!(Viewer::Owner(owner).sees_agent(&all["labels"], |_| None));
    let mine = json!({BOT_CONVERSATION_LABEL:bob});
    assert!(!Viewer::Worker(bobs).sees_agent(&mine, |_| Some(Some(bob))));
}
