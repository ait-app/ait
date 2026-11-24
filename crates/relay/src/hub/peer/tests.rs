use serde_json::json;

use super::*;

fn decoded(reply: &Bytes) -> Value {
    serde_json::from_slice(reply).expect("JSON reply")
}

#[test]
fn hellos_must_declare_this_version() {
    assert_eq!(
        serde_json::from_slice::<Value>(HELLO).expect("JSON hello"),
        json!({ "v": VERSION })
    );
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
    assert_eq!(reply["error"]["code"], -32_000);
    assert_eq!(reply["error"]["code"], TOO_LARGE);
    assert_eq!(reply["error"]["message"], TOO_LARGE_MESSAGE);
    assert!(reply.get("result").is_none());
}

#[test]
fn refusals_map_to_reply_errors() {
    let (code, message) = refusal(PeerRefusal::Disabled);
    assert_eq!(code, -32_001);
    assert_eq!(code, NOT_ACCEPTING);
    assert_eq!(message, "this host does not accept calls from other hosts");
    assert_eq!(
        refusal(PeerRefusal::Failed {
            code: -32_602,
            message: "Unknown or unavailable tool".to_owned(),
        }),
        (-32_602, "Unknown or unavailable tool".to_owned())
    );
}

#[test]
fn requests_encode_to_what_the_receiver_decodes() {
    let request = Request {
        tool: "agent_get".to_owned(),
        arguments: json!({"agentId":"b"}),
        caller: Some("a".to_owned()),
    };
    let encoded = encode_request(7, request.clone());
    assert_eq!(
        decoded(&encoded),
        json!({"id":7,"tool":"agent_get","arguments":{"agentId":"b"},"caller":{"agentId":"a"}})
    );
    assert_eq!(decode_request(&encoded), Ok((7, request)));
    let anonymous = encode_request(
        8,
        Request {
            tool: "agent_list".to_owned(),
            arguments: json!({}),
            caller: None,
        },
    );
    assert_eq!(
        decoded(&anonymous),
        json!({"id":8,"tool":"agent_list","arguments":{}})
    );
}

#[test]
fn reply_errors_map_to_call_errors() {
    assert_eq!(
        decode_reply(&encode_reply(3, Ok(json!({"content":[]})))),
        Ok((3, Ok(json!({"content":[]}))))
    );
    let error = |code: i32| encode_reply(4, Err((code, "no".to_owned())));
    assert_eq!(
        decode_reply(&error(NOT_ACCEPTING)),
        Ok((4, Err(Error::NotAccepting)))
    );
    assert_eq!(
        decode_reply(&error(TOO_LARGE)),
        Ok((4, Err(Error::TooLarge)))
    );
    assert_eq!(
        decode_reply(&error(-32_602)),
        Ok((
            4,
            Err(Error::Rejected {
                code: -32_602,
                message: "no".to_owned()
            })
        ))
    );
    for text in [
        br#"{"id":1}"#.as_slice(),
        br#"{"result":{}}"#,
        br#"{"id":1,"error":{"code":"x","message":"no"}}"#,
        br#"{"id":1,"result":[]}"#,
        br#"{"id":1,"result":"done"}"#,
        br#"{"id":1,"tool":"agent_list","arguments":{}}"#,
        b"not json",
    ] {
        assert_eq!(
            decode_reply(text),
            Err(Malformed),
            "{}",
            String::from_utf8_lossy(text)
        );
    }
}

#[test]
fn a_receiver_that_cannot_run_a_call_now_is_a_rejection_with_its_reason() {
    // `-32002`: the receiving Host's call budget is full or it is draining, so the call did not
    // run. Unlike `TOO_LARGE`, the caller keeps the receiver's reason.
    for reason in ["MCP request budget exhausted", "Server is draining"] {
        assert_eq!(
            decode_reply(&encode_reply(5, Err((-32_002, reason.to_owned())))),
            Ok((
                5,
                Err(Error::Rejected {
                    code: -32_002,
                    message: reason.to_owned()
                })
            ))
        );
    }
}

#[test]
fn hosts_without_an_address_are_left_out_and_unknown_states_read_as_offline() {
    let (host_id, node_id, server_id) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let listed = json!({"hosts":[
        {"host_id":host_id,"node_id":node_id,"server_id":server_id,"name":"Studio",
            "hostname":null,"platform":"macos","app_version":"1.2.3","state":"sleeping",
            "peer":{"supported":true,"enabled":true},"later":"ignored"},
        {"host_id":Uuid::new_v4(),"node_id":null,"server_id":Uuid::new_v4(),"name":"New",
            "hostname":null,"platform":null,"app_version":null,"state":"offline",
            "peer":{"supported":false,"enabled":false}},
        {"host_id":Uuid::new_v4(),"node_id":Uuid::new_v4(),"server_id":null,"name":"Old",
            "hostname":null,"platform":null,"app_version":null,"state":"online",
            "peer":{"supported":false,"enabled":false}},
    ]});
    assert_eq!(
        decode_hosts(listed),
        Ok(vec![Host {
            server_id,
            host_id,
            node_id,
            name: "Studio".to_owned(),
            hostname: String::new(),
            platform: "macos".to_owned(),
            app_version: "1.2.3".to_owned(),
            state: HostState::Offline,
            supported: true,
            enabled: true,
        }])
    );
    assert_eq!(decode_hosts(json!({"hosts":{}})), Err(Malformed));
    assert_eq!(decode_hosts(json!({})), Err(Malformed));
}

#[tokio::test(start_paused = true)]
async fn a_host_list_is_reused_for_five_seconds_on_its_logical_link() {
    let link = Some(Uuid::new_v4());
    let listed = Listed {
        link,
        at: Instant::now(),
        hosts: Vec::new(),
    };
    assert_eq!(listed.current(link), Some([].as_slice()));
    // A new logical link, or none, reads the list again.
    assert_eq!(listed.current(Some(Uuid::new_v4())), None);
    assert_eq!(listed.current(None), None);
    tokio::time::advance(Duration::from_millis(4_999)).await; // virtual time
    assert_eq!(listed.current(link), Some([].as_slice()));
    tokio::time::advance(Duration::from_millis(1)).await; // virtual time
    assert_eq!(listed.current(link), None);
}
