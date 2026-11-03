use serde_json::json;

use super::*;
use crate::host::hub::tests::scope;

fn update(agent: &str, seq: u64) -> Marker {
    Marker {
        agent: agent.to_owned(),
        publication: json!({"method":"agent.timeline.update",
            "params":{"agentId":agent,"epoch":"e","seq":seq}}),
    }
}

fn reset(agent: &str) -> Marker {
    Marker {
        agent: agent.to_owned(),
        publication: json!({"method":"agent.timeline.update",
            "params":{"agentId":agent,"epoch":"e","reset":true}}),
    }
}

#[test]
fn newer_updates_replace_older_ones_and_resets_supersede_earlier_markers() {
    let scope = scope();
    let mut markers = Markers::default();
    for seq in 1..=50 {
        markers.insert(&scope, update("a", seq));
    }
    assert_eq!(markers.take(&scope), Some(update("a", 50)));
    assert_eq!(markers.take(&scope), None);
    markers.insert(&scope, update("a", 51));
    markers.insert(&scope, reset("a"));
    markers.insert(&scope, update("a", 52));
    assert_eq!(markers.take(&scope), Some(reset("a")));
    assert_eq!(markers.take(&scope), Some(update("a", 52)));
    assert_eq!(markers.take(&scope), None);
}

#[test]
fn failed_deliveries_return_unless_superseded() {
    let scope = scope();
    let mut markers = Markers::default();
    markers.insert(&scope, update("a", 1));
    let failed = markers.take(&scope).unwrap();
    markers.restore(&scope, failed.clone());
    assert_eq!(markers.take(&scope), Some(failed));
    markers.insert(&scope, update("a", 2));
    let older = markers.take(&scope).unwrap();
    markers.insert(&scope, update("a", 3));
    markers.restore(&scope, older);
    assert_eq!(markers.take(&scope), Some(update("a", 3)));
    markers.insert(&scope, reset("a"));
    let in_flight = markers.take(&scope).unwrap();
    markers.insert(&scope, update("a", 4));
    markers.restore(&scope, in_flight);
    assert_eq!(markers.take(&scope), Some(reset("a")));
    assert_eq!(markers.take(&scope), Some(update("a", 4)));
}

#[test]
fn agents_take_turns_and_scope_changes_discard_markers() {
    let scope = scope();
    let mut markers = Markers::default();
    markers.insert(&scope, update("a", 1));
    markers.insert(&scope, update("b", 1));
    assert_eq!(markers.take(&scope).unwrap().agent, "a");
    markers.insert(&scope, update("a", 2));
    assert_eq!(markers.take(&scope).unwrap().agent, "b");
    assert_eq!(markers.take(&scope).unwrap().agent, "a");
    markers.insert(&scope, update("a", 3));
    let other = crate::host::hub::tests::scope();
    assert_eq!(markers.take(&other), None);
    markers.insert(&other, update("b", 2));
    assert_eq!(markers.take(&scope), None);
    assert_eq!(markers.take(&other), Some(update("b", 2)));
    markers.restore(&scope, update("a", 4));
    assert_eq!(markers.take(&other), None);
}
