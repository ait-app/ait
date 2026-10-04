use super::{Fault, OpenCodeExecutionLimits, ProgressEvent, Stream, Version};
use serde_json::{Value, json};

fn stream() -> Stream {
    Stream::new("ses_one", "msg_input", OpenCodeExecutionLimits::default())
}

fn message(role: &str) -> Value {
    json!({"type":"message.updated","properties":{"info":{"id":"msg_a","sessionID":"ses_one","role":role}}})
}

fn part(text: Value) -> Value {
    let mut event = json!({"type":"message.part.updated","properties":{"part":{"id":"prt_a","messageID":"msg_a","sessionID":"ses_one","type":"text"}}});
    event["properties"]["part"]["text"] = text;
    event
}

fn delta(text: &str) -> Value {
    json!({"type":"message.part.delta","properties":{"sessionID":"ses_one","messageID":"msg_a","partID":"prt_a","field":"text","delta":text}})
}

fn texts(stream: &mut Stream, event: &Value) -> Vec<String> {
    stream
        .observe(event, Version::V1)
        .unwrap()
        .into_iter()
        .map(|event| match event {
            ProgressEvent::TextDelta { delta, .. } => delta,
            ProgressEvent::Timeline(_) => panic!("text materialization does not emit history"),
        })
        .collect()
}

#[test]
fn legal_empty_placeholders_deltas_and_final_snapshots_emit_each_byte_once() {
    let mut stream = stream();
    assert!(texts(&mut stream, &message("assistant")).is_empty());
    assert!(texts(&mut stream, &part(json!(""))).is_empty());
    assert!(texts(&mut stream, &delta("")).is_empty());
    assert_eq!(texts(&mut stream, &delta("你好")), ["你好"]);
    assert!(texts(&mut stream, &part(json!(""))).is_empty());
    assert_eq!(texts(&mut stream, &delta(" world")), [" world"]);
    assert!(texts(&mut stream, &part(json!("你好 world"))).is_empty());
    assert!(texts(&mut stream, &message("assistant")).is_empty());
}

#[test]
fn out_of_order_roles_buffer_assistant_text_and_discard_user_text() {
    for role in ["assistant", "user"] {
        let mut stream = stream();
        assert!(texts(&mut stream, &part(json!(""))).is_empty());
        assert!(texts(&mut stream, &delta("answer")).is_empty());
        let output = texts(&mut stream, &message(role));
        if role == "assistant" {
            assert_eq!(output, ["answer"]);
            assert!(texts(&mut stream, &part(json!("answer"))).is_empty());
        } else {
            assert!(output.is_empty());
            assert!(texts(&mut stream, &part(json!("answer"))).is_empty());
            assert_eq!(stream.bytes, 0);
            assert!(stream.parts.is_empty());
        }
    }
    let mut stream = stream();
    let mut input = part(json!("hello"));
    input["properties"]["part"]["messageID"] = json!("msg_input");
    assert!(texts(&mut stream, &input).is_empty());
}

#[test]
fn text_still_requires_a_string_and_nonempty_native_identities() {
    for invalid in [Value::Null, json!(42), json!({})] {
        let mut stream = stream();
        texts(&mut stream, &message("assistant"));
        assert_eq!(
            stream
                .observe(&part(invalid), Version::V1)
                .unwrap_err()
                .code,
            Fault::ProviderFailed
        );
    }
    let mut stream = stream();
    let mut missing = part(json!(""));
    missing["properties"]["part"]
        .as_object_mut()
        .unwrap()
        .remove("text");
    assert!(stream.observe(&missing, Version::V1).is_err());
    let mut invalid = part(json!("text"));
    invalid["properties"]["part"]["id"] = json!("");
    assert!(stream.observe(&invalid, Version::V1).is_err());
}

#[test]
fn unknown_part_types_and_foreign_sessions_cannot_become_assistant_deltas() {
    let mut stream = stream();
    texts(&mut stream, &message("assistant"));
    assert!(texts(&mut stream, &delta("unknown type")).is_empty());
    let mut reasoning = part(json!("thought"));
    reasoning["properties"]["part"]["type"] = json!("reasoning");
    assert!(texts(&mut stream, &reasoning).is_empty());
    assert!(texts(&mut stream, &delta("reasoning")).is_empty());
    let mut foreign = part(json!("foreign"));
    foreign["properties"]["part"]["sessionID"] = json!("ses_other");
    assert!(texts(&mut stream, &foreign).is_empty());
    let mut unknown = message("system");
    assert!(texts(&mut stream, &unknown).is_empty());
    unknown["type"] = json!("server.heartbeat");
    assert!(texts(&mut stream, &unknown).is_empty());
    assert_eq!(
        texts(&mut stream, &json!({"payload":part(json!("answer"))})),
        ["answer"]
    );
}

#[test]
fn changed_roles_parents_and_published_text_fail_without_relabeling_history() {
    let mut stream = stream();
    texts(&mut stream, &message("assistant"));
    texts(&mut stream, &part(json!("answer")));
    assert_eq!(
        stream
            .observe(&message("user"), Version::V1)
            .unwrap_err()
            .code,
        Fault::RunRecoveryFailed
    );
    assert_eq!(
        stream
            .observe(&part(json!("changed")), Version::V1)
            .unwrap_err()
            .code,
        Fault::RunRecoveryFailed
    );
    let mut wrong = part(json!("answer more"));
    wrong["properties"]["part"]["messageID"] = json!("msg_other");
    assert_eq!(
        stream.observe(&wrong, Version::V1).unwrap_err().code,
        Fault::ProviderFailed
    );
    let mut wrong = delta("more");
    wrong["properties"]["messageID"] = json!("msg_other");
    assert_eq!(
        stream.observe(&wrong, Version::V1).unwrap_err().code,
        Fault::ProviderFailed
    );
}

#[test]
fn observation_limits_allow_updates_at_capacity_and_reject_new_resources() {
    let limits = OpenCodeExecutionLimits {
        max_steps: 1,
        max_output_bytes: 6,
        ..Default::default()
    };
    let mut stream = Stream::new("ses_one", "msg_input", limits);
    texts(&mut stream, &message("assistant"));
    texts(&mut stream, &part(json!("")));
    assert_eq!(texts(&mut stream, &delta("answer")), ["answer"]);
    assert!(texts(&mut stream, &part(json!("answer"))).is_empty());
    assert_eq!(
        stream.observe(&delta("!"), Version::V1).unwrap_err().code,
        Fault::RunLimitExceeded
    );
    let mut another = part(json!(""));
    another["properties"]["part"]["id"] = json!("prt_other");
    assert_eq!(
        stream.observe(&another, Version::V1).unwrap_err().code,
        Fault::RunLimitExceeded
    );
    let mut another = message("assistant");
    another["properties"]["info"]["id"] = json!("msg_other");
    assert_eq!(
        stream.observe(&another, Version::V1).unwrap_err().code,
        Fault::RunLimitExceeded
    );
}

#[test]
fn v2_empty_text_deltas_are_noops_but_missing_or_nonstring_text_is_rejected() {
    let mut stream = stream();
    let mut event = json!({"type":"session.text.delta","data":{"sessionID":"ses_one","assistantMessageID":"a1","delta":""}});
    assert!(stream.observe(&event, Version::V2).unwrap().is_empty());
    event["data"]["delta"] = json!("answer");
    assert!(
        matches!(stream.observe(&event, Version::V2).unwrap().as_slice(), [ProgressEvent::TextDelta {delta, ..}] if delta == "answer")
    );
    event["data"]["delta"] = Value::Null;
    assert_eq!(
        stream.observe(&event, Version::V2).unwrap_err().code,
        Fault::ProviderFailed
    );
}
