use serde_json::json;

use super::*;

fn decoded(reply: &Bytes) -> Value {
    serde_json::from_slice(reply).expect("JSON reply")
}

#[test]
fn hellos_must_declare_this_version() {
    assert_eq!(decode_hello(HELLO), Ok(()));
    assert_eq!(decode_hello(br#"{"v":1,"note":"later"}"#), Ok(()));
    for payload in [
        br#"{"v":2}"#.as_slice(),
        br#"{"v":"1"}"#,
        b"{}",
        b"",
        b"hello",
    ] {
        assert_eq!(decode_hello(payload), Err(Malformed), "{payload:?}");
    }
}

#[test]
fn requests_decode_with_and_without_a_caller() {
    let (id, request) = decode_request(
        br#"{"id":7,"tool":"agent_get","arguments":{"agentId":"b"},"caller":{"agentId":"a"}}"#,
    )
    .expect("request");
    assert_eq!(id, 7);
    assert_eq!(
        request,
        Request {
            tool: "agent_get".to_owned(),
            arguments: json!({"agentId":"b"}),
            caller: Some("a".to_owned()),
        }
    );
    let (_, request) =
        decode_request(br#"{"id":8,"tool":"agent_list","arguments":{}}"#).expect("request");
    assert_eq!(request.caller, None);
}

#[test]
fn messages_that_are_not_requests_are_malformed() {
    for text in [
        br#"{"tool":"agent_list","arguments":{}}"#.as_slice(),
        br#"{"id":-1,"tool":"agent_list","arguments":{}}"#,
        br#"{"id":1,"arguments":{}}"#,
        br#"{"id":1,"tool":"agent_list"}"#,
        br#"{"id":1,"tool":"agent_list","arguments":[]}"#,
        br#"{"id":1,"tool":"agent_list","arguments":{},"caller":{}}"#,
        br#"{"id":1,"result":{}}"#,
        b"not json",
    ] {
        assert_eq!(
            decode_request(text),
            Err(Malformed),
            "{}",
            String::from_utf8_lossy(text)
        );
    }
}

#[test]
fn replies_carry_the_result_or_the_error() {
    assert_eq!(
        decoded(&encode_reply(3, Ok(json!({"content":[],"isError":false})))),
        json!({"id":3,"result":{"content":[],"isError":false}})
    );
    assert_eq!(
        decoded(&encode_reply(4, Err((-32_602, "Unknown tool".to_owned())))),
        json!({"id":4,"error":{"code":-32_602,"message":"Unknown tool"}})
    );
}

#[test]
fn replies_over_the_payload_limit_become_errors() {
    let large = json!({"content":[{"type":"text","text":"x".repeat(wire::MAX_PAYLOAD)}]});
    let reply = decoded(&encode_reply(5, Ok(large)));
    assert_eq!(reply["id"], 5);
    assert_eq!(reply["error"]["code"], SERVER_ERROR);
    assert_eq!(reply["error"]["message"], TOO_LARGE);
    assert!(reply.get("result").is_none());
}

#[test]
fn refusals_map_to_reply_errors() {
    let (code, message) = refusal(PeerRefusal::Disabled);
    assert_eq!(code, SERVER_ERROR);
    assert_eq!(message, "this host does not accept calls from other hosts");
    assert_eq!(
        refusal(PeerRefusal::Failed {
            code: -32_602,
            message: "Unknown or unavailable tool".to_owned(),
        }),
        (-32_602, "Unknown or unavailable tool".to_owned())
    );
}
