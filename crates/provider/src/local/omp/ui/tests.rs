use super::*;

#[test]
fn completed_dictation_delivers_a_draft_but_partial_or_failed_capture_does_not() {
    let mut state = State::default();
    state
        .capture(&json!({"type":"dictation_transcript","captureId":"one","text":"Partial"}))
        .unwrap();
    assert!(state.snapshot()["editor"].is_null());
    state
        .capture(&json!({"type":"dictation_end","captureId":"one","text":"Transcript"}))
        .unwrap();
    let editor = state.snapshot()["editor"].clone();
    assert_eq!(editor["text"], "Transcript");
    state.capture(&json!({"type":"dictation_end","captureId":"two","text":"Incomplete","error":"Capture failed"})).unwrap();
    assert_eq!(state.snapshot()["editor"], editor);
}

#[test]
fn native_widgets_update_and_clear_without_chat_history() {
    let mut state = State::default();
    state
        .capture(&json!({"method":"setStatus","statusKey":"build","statusText":"Ready"}))
        .unwrap();
    state.capture(&json!({"method":"setWidget","widgetKey":"tasks","widgetLines":["one"],"widgetPlacement":"belowEditor"})).unwrap();
    state
        .capture(&json!({"method":"setWidget","widgetKey":"tasks","widgetLines":["two"]}))
        .unwrap();
    assert_eq!(
        state.snapshot()["widgets"],
        json!([{"key":"tasks","lines":["two"],"placement":"aboveEditor"}])
    );
    state
        .capture(&json!({"method":"setStatus","statusKey":"build"}))
        .unwrap();
    state
        .capture(&json!({"method":"setWidget","widgetKey":"tasks"}))
        .unwrap();
    assert_eq!(state.snapshot()["statuses"], json!([]));
    assert_eq!(state.snapshot()["widgets"], json!([]));
}

#[test]
fn editor_title_and_login_link_are_transient_and_validated() {
    let mut state = State::default();
    state
        .capture(&json!({"id":"editor-1","method":"set_editor_text","text":"Continue here"}))
        .unwrap();
    state
        .capture(&json!({"method":"setTitle","title":"Native title"}))
        .unwrap();
    state.capture(&json!({"method":"open_url","url":"https://example.com/login","instructions":"Enter the code"})).unwrap();
    assert_eq!(state.snapshot()["editor"]["text"], "Continue here");
    assert_eq!(state.snapshot()["title"], "Native title");
    assert_eq!(
        state.snapshot()["link"]["launchUrl"],
        "https://example.com/login"
    );
    assert!(
        state
            .capture(&json!({"method":"open_url","url":"file:///etc/passwd"}))
            .is_err()
    );
    assert!(
        state
            .capture(&json!({"method":"setWidget","widgetKey":"x","widgetLines":[1]}))
            .is_err()
    );
    assert!(
        state
            .capture(&json!({"method":"setStatus","statusKey":"x","statusText":"x".repeat(4097)}))
            .is_err()
    );
    for index in 0..32 {
        state
            .capture(&json!({"method":"setStatus","statusKey":index.to_string(),"statusText":"ok"}))
            .unwrap();
    }
    assert!(
        state
            .capture(&json!({"method":"setStatus","statusKey":"overflow","statusText":"ok"}))
            .is_err()
    );
}
