use super::*;

#[test]
fn capabilities_require_an_authority_but_survive_physical_disconnect() {
    let mut state = LinkState {
        methods: vec!["hub.events.publish".to_owned()],
        ..LinkState::default()
    };
    assert!(!state.supports("hub.events.publish"));
    state.scope = Some(Scope {
        center: "https://example.com/".to_owned(),
        node_session_id: Uuid::new_v4(),
    });
    assert!(state.supports("hub.events.publish"));
    assert!(!state.online);
    assert!(!state.supports("push.register"));
    assert_eq!(
        serde_json::from_value::<Scope>(serde_json::to_value(&state.scope).unwrap()).unwrap(),
        state.scope.unwrap()
    );
}
