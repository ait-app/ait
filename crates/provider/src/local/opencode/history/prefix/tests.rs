use super::*;
use serde_json::json;

fn input() -> Value {
    json!({"info":{"id":"user","role":"user","sessionID":"session","time":{"created":1}},"parts":[{"id":"input","type":"text","messageID":"user","sessionID":"session","text":"hello"}]})
}

fn answer() -> Value {
    json!({"info":{"id":"answer","role":"assistant","sessionID":"session","time":{"created":2}},"parts":[{"id":"text","type":"text","messageID":"answer","sessionID":"session","text":"answer"}]})
}

#[test]
fn missing_admission_fails_but_missing_text_waits_for_durable_history() {
    let messages = [input(), answer()];
    assert_eq!(
        before_text(Version::V1, "session", "missing", "text", &messages)
            .unwrap_err()
            .code,
        Fault::RunRecoveryFailed
    );
    assert!(
        before_text(Version::V1, "session", "user", "missing", &messages)
            .unwrap()
            .is_none()
    );
    let prefix = before_text(Version::V1, "session", "user", "text", &messages)
        .unwrap()
        .unwrap();
    assert_eq!(prefix.len(), 1);
    assert_eq!(prefix[0].id, "user");
    assert!(super::super::normalize(Version::V1, "session", &messages).is_err());
}

#[test]
fn unsettled_predecessors_wait_and_invalid_predecessors_fail() {
    let mut previous = answer();
    previous["info"]["id"] = json!("previous");
    previous["parts"][0]["id"] = json!("previous-text");
    previous["parts"][0]["messageID"] = json!("previous");
    let mut messages = [input(), previous, answer()];
    assert!(
        before_text(Version::V1, "session", "user", "text", &messages)
            .unwrap()
            .is_none()
    );
    messages[1]["info"]["time"]["completed"] = json!(3);
    assert_eq!(
        before_text(Version::V1, "session", "user", "text", &messages)
            .unwrap()
            .unwrap()
            .len(),
        2
    );
    messages[1]["parts"][0]["messageID"] = json!("wrong");
    assert_eq!(
        before_text(Version::V1, "session", "user", "text", &messages)
            .unwrap_err()
            .code,
        Fault::ProviderFailed
    );
}

#[test]
fn unfinished_tool_inside_live_message_is_not_published_as_complete() {
    let mut current = answer();
    let text = current["parts"][0].clone();
    current["parts"] = json!([
        {"id":"tool","messageID":"answer","sessionID":"session","type":"tool","callID":"call","tool":"bash","state":{"status":"running","input":{"command":"pwd"}}},text]);
    assert!(
        before_text(Version::V1, "session", "user", "text", &[input(), current])
            .unwrap()
            .is_none()
    );
}
