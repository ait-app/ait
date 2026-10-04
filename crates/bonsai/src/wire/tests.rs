use serde_json::{Value, json};

use super::{
    BODY_BYTES, Batch, EVENT_BYTES, EncodeError, Execution, Inbound, Incoming, ReasonCode,
    RunState, Status, Unavailable, decode, display, events_frames, is_model_id, is_token,
    new_epoch, status, truncate, unavailable,
};

fn head_of(frame: &str) -> Value {
    let head = frame.split_once('\n').map_or(frame, |(head, _)| head);
    serde_json::from_str(head).expect("head is JSON")
}

fn body_of(frame: &str) -> Vec<Value> {
    let body = frame.split_once('\n').map_or("[]", |(_, body)| body);
    serde_json::from_str(body).expect("body is a JSON array")
}

#[test]
fn truncation_stops_on_code_point_boundaries() {
    assert_eq!(truncate("abc", 3), ("abc", false));
    assert_eq!(truncate("abcd", 3), ("abc", true));
    // "中" is three bytes; cutting inside it backs off to the previous boundary.
    assert_eq!(truncate("a中b", 2), ("a", true));
    assert_eq!(truncate("a中b", 4), ("a中", true));
    assert_eq!(truncate("", 0), ("", false));
}

#[test]
fn display_strings_drop_controls_and_fit() {
    assert_eq!(display("  a\u{0}b\nc  ", 128), "abc");
    assert_eq!(display("中文名字", 7), "中文");
}

#[test]
fn identifier_shapes_follow_the_protocol() {
    assert!(is_token("prj_0123abcd", 64));
    assert!(is_token("a.b_c:d-e", 64));
    assert!(!is_token("", 64));
    assert!(!is_token("has space", 64));
    assert!(!is_token(&"x".repeat(65), 64));
    assert!(is_token(&"x".repeat(128), 128));
    assert!(is_model_id("claude-opus-4-1[1m]"));
    assert!(!is_model_id("Opus 4"));
    assert!(!is_model_id("模型"));
    assert!(!is_model_id(&"m".repeat(129)));
}

#[test]
fn heartbeat_and_unknown_frames_decode_without_errors() {
    assert_eq!(decode("pong").expect("pong"), Incoming::Pong);
    assert_eq!(
        decode(r#"{"type":"hub.future","x":1}"#).expect("unknown type"),
        Incoming::Frame(Inbound::Unknown)
    );
    assert!(decode("not json").is_err());
    assert!(decode(r#"{"no":"type"}"#).is_err());
}

#[test]
fn dispatch_decodes_with_defaults_and_ignores_extra_fields() {
    let frame = json!({
        "type": "run.dispatch", "run_id": "r_1", "space_id": "sandbox",
        "task": {"path": "1_Projects/X/X.md", "line": 42, "line_hash": "sha256:00",
                 "text": "- [ ] do it", "heading": null, "context": "", "context_truncated": false},
        "project": {"id": "prj_1"}, "provider": null, "model": null,
        "bonsai": {"mcp_url": "http://localhost:8860/mcp"},
        "requested_by": {"id": "github:2", "login": "member", "owner": false},
        "session": "bonsai.session/1", "future_field": true
    });
    let Incoming::Frame(Inbound::Dispatch(dispatch)) =
        decode(&frame.to_string()).expect("dispatch decodes")
    else {
        panic!("expected a dispatch frame");
    };
    assert_eq!(dispatch.run_id, "r_1");
    assert_eq!(dispatch.task.line, 42);
    assert!(dispatch.settings.is_empty());
    assert_eq!(dispatch.instruction, "");
    assert_eq!(dispatch.wrapup, "");
    assert!(!dispatch.requested_by.owner);
}

#[test]
fn known_frames_with_wrong_fields_are_reported_with_their_run() {
    let decoded = decode(r#"{"type":"run.dispatch","run_id":"r_9","task":7}"#).expect("json");
    assert_eq!(
        decoded,
        Incoming::Malformed {
            kind: "run.dispatch".to_owned(),
            run_id: Some("r_9".to_owned())
        }
    );
}

#[test]
fn session_frames_decode() {
    let subscribe = decode(
        r#"{"type":"session.subscribe","run_id":"r_1","sub":"abc","after":{"epoch":"e1","seq":-1}}"#,
    )
    .expect("subscribe");
    assert!(matches!(
        subscribe,
        Incoming::Frame(Inbound::Subscribe { ref after, .. }) if after.as_ref().is_some_and(|c| c.seq == -1)
    ));
    let fresh = decode(r#"{"type":"session.subscribe","run_id":"r_1","sub":"abc","after":null}"#)
        .expect("subscribe from start");
    assert!(matches!(
        fresh,
        Incoming::Frame(Inbound::Subscribe { after: None, .. })
    ));
    let answer = decode(
        r#"{"type":"session.answer","run_id":"r_1","by":{"id":"github:2","login":"m"},"ask_id":"a1","option_id":"allow","answers":{"q0":["x"]}}"#,
    )
    .expect("answer");
    assert!(
        matches!(answer, Incoming::Frame(Inbound::Answer(ref a)) if a.answers.is_some() && a.note.is_none())
    );
    let send = decode(
        r#"{"type":"session.send","run_id":"r_1","by":{"id":"github:2"},"input_id":"0123","text":"hi"}"#,
    )
    .expect("send");
    assert!(matches!(send, Incoming::Frame(Inbound::Send { ref by, .. }) if by.login.is_none()));
}

#[test]
fn status_heads_omit_absent_fields() {
    let claimed = Status {
        kind: "run.status",
        run_id: "r_1".to_owned(),
        status: RunState::Claimed,
        at: 1,
        execution: Some(Execution {
            provider: "claude".to_owned(),
            model: None,
            approvals: true,
            bonsai_write: false,
        }),
        reason_code: None,
        reason_detail: None,
        final_text: None,
    };
    let head = head_of(&status(&claimed).expect("encodes"));
    assert_eq!(
        head,
        json!({"type": "run.status", "run_id": "r_1", "status": "claimed", "at": 1,
               "execution": {"provider": "claude", "model": null, "approvals": true, "bonsai_write": false}})
    );
    let failed = Status {
        status: RunState::Failed,
        execution: None,
        reason_code: Some(ReasonCode::RunUnknown),
        ..claimed
    };
    let head = head_of(&status(&failed).expect("encodes"));
    assert_eq!(head["reason_code"], "run_unknown");
    assert!(head.get("execution").is_none());
}

#[test]
fn oversized_status_texts_are_halved_until_the_head_fits() {
    // Arrange: 4 KiB of control bytes escapes to about 24 KiB.
    let large = Status {
        kind: "run.status",
        run_id: "r_1".to_owned(),
        status: RunState::Failed,
        at: 1,
        execution: None,
        reason_code: Some(ReasonCode::ProviderError),
        reason_detail: Some("\u{1}".repeat(1024)),
        final_text: Some("\u{2}".repeat(4096)),
    };

    // Act
    let frame = status(&large).expect("a fitted status encodes");

    // Assert
    assert!(frame.len() <= 16 * 1024);
    let head = head_of(&frame);
    let final_text = head["final_text"].as_str().expect("final text kept");
    assert!(!final_text.is_empty() && final_text.len() < 4096);
    assert_eq!(head["reason_code"], "provider_error");
}

#[test]
fn a_status_that_cannot_fit_even_without_texts_is_refused() {
    let large = Status {
        kind: "run.status",
        run_id: "r".repeat(20_000),
        status: RunState::Completed,
        at: 1,
        execution: None,
        reason_code: None,
        reason_detail: None,
        final_text: Some("x".repeat(100)),
    };
    assert_eq!(status(&large), Err(EncodeError::Head { limit: 16 * 1024 }));
}

#[test]
fn unavailable_carries_exactly_one_address() {
    let by_sub = head_of(&unavailable("r_1", Unavailable::Sub("s1")).expect("encodes"));
    assert_eq!(by_sub["sub"], "s1");
    assert!(by_sub.get("ref").is_none());
    assert_eq!(by_sub["reason"], "no_history");
    let by_ref = head_of(&unavailable("r_1", Unavailable::Ref("i1")).expect("encodes"));
    assert_eq!(by_ref["ref"], "i1");
    assert!(by_ref.get("sub").is_none());
}

#[test]
fn an_empty_answer_is_one_synced_frame() {
    let frames = events_frames(
        Batch {
            run_id: "r_1",
            epoch: "e1",
            first: 5,
            sub: Some("s"),
            reset: false,
        },
        &[],
    )
    .expect("encodes");
    assert_eq!(frames.len(), 1);
    let head = head_of(&frames[0].text);
    assert_eq!(head["first"], 5);
    assert_eq!(head["last"], 4);
    assert_eq!((frames[0].first, frames[0].last), (5, 4));
    assert_eq!(head["sync"], true);
    assert!(head.get("reset").is_none());
    assert!(body_of(&frames[0].text).is_empty());
}

#[test]
fn replay_frames_are_consecutive_and_bounded() {
    let events: Vec<String> = (0..40)
        .map(|seq| {
            json!({"seq": seq, "at": 0, "t": "text", "mid": "m", "text": "y".repeat(20_000)})
                .to_string()
        })
        .collect();
    let frames = events_frames(
        Batch {
            run_id: "r_1",
            epoch: "e1",
            first: 0,
            sub: Some("s"),
            reset: true,
        },
        &events,
    )
    .expect("encodes");
    assert!(frames.len() > 1);
    let mut expected = 0;
    for (index, encoded) in frames.iter().enumerate() {
        let frame = &encoded.text;
        assert!(frame.len() <= super::FRAME_BYTES);
        let head = head_of(frame);
        let body = body_of(frame);
        assert_eq!(head["first"], expected);
        assert_eq!(head["last"], expected + body.len() as u64 - 1);
        assert_eq!(encoded.first, expected);
        let next = i64::try_from(expected + body.len() as u64).expect("small");
        assert_eq!(encoded.last, next - 1);
        assert_eq!(head.get("reset").is_some(), index == 0);
        assert_eq!(head.get("sync").is_some(), index == frames.len() - 1);
        for event in &body {
            assert_eq!(event["seq"], expected);
            expected += 1;
        }
        let body_bytes = frame.len() - frame.find('\n').expect("has body") - 1;
        assert!(body_bytes <= BODY_BYTES + 2);
    }
    assert_eq!(expected, 40);
}

#[test]
fn live_frames_never_carry_answer_markers() {
    let frames = events_frames(
        Batch {
            run_id: "r_1",
            epoch: "e1",
            first: 0,
            sub: None,
            reset: true,
        },
        &[json!({"seq": 0, "at": 0, "t": "closed"}).to_string()],
    )
    .expect("encodes");
    let head = head_of(&frames[0].text);
    assert!(head.get("sub").is_none());
    assert!(head.get("reset").is_none());
    assert!(head.get("sync").is_none());
}

#[test]
fn an_oversized_stored_event_goes_out_as_a_notice_with_its_seq() {
    // Arrange
    let big = json!({"seq": 8, "at": 42, "t": "text", "mid": "m", "text": "z".repeat(EVENT_BYTES)})
        .to_string();
    let small = json!({"seq": 9, "at": 43, "t": "closed"}).to_string();
    let batch = Batch {
        run_id: "r",
        epoch: "e",
        first: 8,
        sub: Some("s"),
        reset: false,
    };

    // Act
    let frames = events_frames(batch, &[big, small]).expect("still encodes");

    // Assert: one synced frame; the bad event is replaced, the next one kept.
    assert_eq!(frames.len(), 1);
    assert_eq!(head_of(&frames[0].text)["sync"], true);
    let body = body_of(&frames[0].text);
    assert_eq!(body[0]["seq"], 8);
    assert_eq!(body[0]["at"], 42);
    assert_eq!(body[0]["t"], "notice");
    assert_eq!(body[0]["level"], "error");
    assert_eq!(body[1]["t"], "closed");
}

#[test]
fn new_epoch_frames_are_empty_live_frames_from_zero() {
    let frame = new_epoch("r_1", "e-2").expect("encodes");
    let head = head_of(&frame);
    assert_eq!(head["epoch"], "e-2");
    assert_eq!(head["first"], 0);
    assert_eq!(head["last"], -1);
    assert!(head.get("sub").is_none());
}

#[test]
fn run_state_spellings_round_trip() {
    for state in [
        RunState::Claimed,
        RunState::Running,
        RunState::Completed,
        RunState::Failed,
        RunState::Cancelled,
    ] {
        assert_eq!(RunState::parse(state.as_str()), Some(state));
    }
    assert!(RunState::Claimed < RunState::Running);
    assert!(RunState::Cancelled.is_terminal());
    assert!(!RunState::Running.is_terminal());
    for code in [
        ReasonCode::ProjectUnavailable,
        ReasonCode::ProviderUnavailable,
        ReasonCode::ProviderError,
        ReasonCode::SessionLost,
        ReasonCode::RunUnknown,
        ReasonCode::Rejected,
    ] {
        assert_eq!(ReasonCode::parse(code.as_str()), Some(code));
        assert_eq!(
            serde_json::to_value(code).expect("serializes"),
            Value::from(code.as_str())
        );
    }
}

#[test]
fn run_ids_are_r_and_32_lowercase_hex_digits() {
    assert!(super::is_run_id("r_0123456789abcdef0123456789abcdef"));
    for bad in [
        "r_0123456789ABCDEF0123456789ABCDEF",
        "r_0123456789abcdef0123456789abcde",
        "r_0123456789abcdef0123456789abcdef0",
        "x_0123456789abcdef0123456789abcdef",
        "r_0123456789abcdef0123456789abcdeg",
        "",
    ] {
        assert!(!super::is_run_id(bad), "{bad}");
    }
}
