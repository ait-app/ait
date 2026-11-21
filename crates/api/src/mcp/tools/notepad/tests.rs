use std::collections::BTreeSet;

use bot::mcp::{Caller, CallerKind, Grant};
use domain::bot::permissions::BotPermissions;
use serde_json::json;

use super::{AUTHOR_TITLE_CHARS, author};
use crate::mcp::Scope;

fn scope(kind: CallerKind) -> Scope {
    Scope {
        caller: Caller {
            root: "wks_root".to_owned(),
            kind,
        },
        grant: Grant {
            permissions: BotPermissions::default(),
            workspaces: BTreeSet::new(),
        },
    }
}

#[test]
fn writes_are_signed_by_the_credential_or_the_named_agent() {
    let bot = scope(CallerKind::Bot {
        bot_id: domain::BotId::generate(),
        conversation_id: domain::ConversationId::generate(),
        session_id: "session".to_owned(),
    });
    let subtree = scope(CallerKind::Subtree {
        agent_id: "child".to_owned(),
        ticket: None,
    });
    let agent = json!({"id":"child","title":"Backend"});
    assert_eq!(author(Some(&bot), None), json!({"kind":"bot"}));
    assert_eq!(
        author(Some(&subtree), Some(&agent)),
        json!({"kind":"agent","agentId":"child","title":"Backend"})
    );
    assert_eq!(author(None, Some(&agent))["kind"], "agent");
    assert_eq!(author(None, None), json!({"kind":"external"}));
    assert_eq!(author(None, Some(&json!({}))), json!({"kind":"external"}));
}

#[test]
fn author_titles_are_bounded_and_free_of_control_characters() {
    let long = json!({"id":"a","title":format!("line\none{}", "x".repeat(200))});
    let signed = author(None, Some(&long));
    let title = signed["title"].as_str().unwrap();
    assert!(title.starts_with("lineone"));
    assert_eq!(title.chars().count(), AUTHOR_TITLE_CHARS);
    for untitled in [json!({"id":"a"}), json!({"id":"a","title":"\n"})] {
        assert_eq!(
            author(None, Some(&untitled)),
            json!({"kind":"agent","agentId":"a"})
        );
    }
}
