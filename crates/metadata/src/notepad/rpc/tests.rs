use serde_json::{Value, json};

use super::execute;
use crate::notepad::service::tests::{Fixture, fixture};
use crate::support::error::ErrorCode;

fn call(fixture: &Fixture, method: &str, params: Value) -> Result<Value, ErrorCode> {
    execute(
        &fixture.notepads,
        &format!("workspace.notepad.{method}.request"),
        params,
    )
}

fn agent() -> Value {
    json!({"kind":"agent","agentId":"agent-1","title":"Backend"})
}

#[test]
fn a_notepad_reads_empty_until_written_and_names_its_file() {
    let fixture = fixture();
    let file = fixture.file("wks_a");
    let empty = call(&fixture, "get", json!({"workspaceId":"wks_a"})).unwrap();
    assert_eq!(empty["workspaceId"], "wks_a");
    assert_eq!(empty["path"], file.to_str().unwrap());
    assert_eq!(empty["directory"], file.parent().unwrap().to_str().unwrap());
    assert_eq!(empty["text"], "");
    assert!(empty["updatedAt"].is_string());

    let written = call(
        &fixture,
        "write",
        json!({"workspaceId":"wks_a","text":"POST /x","author":agent()}),
    )
    .unwrap();
    assert_eq!(written["accepted"], true);
    assert_eq!(written["path"], empty["path"]);
    assert_ne!(written["revision"], empty["revision"]);
    assert!(written.get("error").is_none());

    let read = call(&fixture, "get", json!({"workspaceId":"wks_a"})).unwrap();
    assert_eq!(
        (&read["text"], &read["revision"]),
        (&json!("POST /x"), &written["revision"])
    );
    assert_eq!(read["updatedAt"], written["updatedAt"]);
    assert_eq!(
        call(&fixture, "get", json!({"workspaceId":"wks_b"})).unwrap()["text"],
        ""
    );
}

#[test]
fn a_stale_replace_is_refused_with_the_current_state_and_an_append_is_signed() {
    let fixture = fixture();
    let first = call(
        &fixture,
        "write",
        json!({"workspaceId":"wks_a","text":"one"}),
    )
    .unwrap();

    for stale in [json!({}), json!({"expected":"0"})] {
        let mut params = json!({"workspaceId":"wks_a","text":"two","author":agent()});
        params
            .as_object_mut()
            .unwrap()
            .extend(stale.as_object().unwrap().clone());
        let refused = call(&fixture, "write", params).unwrap();
        assert_eq!(
            (&refused["accepted"], &refused["revision"]),
            (&json!(false), &first["revision"])
        );
        assert!(refused["error"].as_str().unwrap().contains("read it again"));
    }

    let appended = call(
        &fixture,
        "write",
        json!({"workspaceId":"wks_a","text":"two","mode":"append","author":agent()}),
    )
    .unwrap();
    assert_eq!(appended["accepted"], true);
    let text = std::fs::read_to_string(fixture.file("wks_a")).unwrap();
    assert!(text.starts_with("one\n\n[Backend · "), "{text}");
    assert!(text.ends_with("]\ntwo\n"), "{text}");

    let unsigned = call(
        &fixture,
        "write",
        json!({"workspaceId":"wks_b","text":"note","mode":"append"}),
    )
    .unwrap();
    assert_eq!(unsigned["accepted"], true);
    let text = std::fs::read_to_string(fixture.file("wks_b")).unwrap();
    assert!(text.starts_with("[User · "), "{text}");

    let cleared = call(
        &fixture,
        "write",
        json!({"workspaceId":"wks_a","text":"","expected":appended["revision"]}),
    )
    .unwrap();
    assert_eq!(cleared["accepted"], true);
    assert_eq!(
        call(&fixture, "get", json!({"workspaceId":"wks_a"})).unwrap()["text"],
        ""
    );
}

#[test]
fn an_oversized_notepad_refuses_with_guidance() {
    let fixture = fixture();
    let empty = call(&fixture, "get", json!({"workspaceId":"wks_a"})).unwrap();
    let large = "x".repeat(256 * 1024 + 1);
    let refused = call(
        &fixture,
        "write",
        json!({"workspaceId":"wks_a","text":large}),
    )
    .unwrap();
    assert_eq!(
        (&refused["accepted"], &refused["revision"]),
        (&json!(false), &empty["revision"])
    );
    assert!(refused["error"].as_str().unwrap().contains("256 KiB"));
}

#[test]
fn invalid_requests_and_unknown_workspaces_are_errors() {
    let fixture = fixture();
    let cases = [
        (
            "get",
            json!({"workspaceId":"wks_missing"}),
            ErrorCode::WorkspaceNotFound,
        ),
        (
            "write",
            json!({"workspaceId":"wks_missing","text":"t"}),
            ErrorCode::WorkspaceNotFound,
        ),
        (
            "get",
            json!({"workspaceId":"wks_a","extra":1}),
            ErrorCode::InvalidMessage,
        ),
        (
            "write",
            json!({"workspaceId":"wks_a"}),
            ErrorCode::InvalidMessage,
        ),
        (
            "write",
            json!({"workspaceId":"wks_a","text":"t","expected":0}),
            ErrorCode::InvalidMessage,
        ),
        (
            "write",
            json!({"workspaceId":"wks_a","text":"a\u{0}b"}),
            ErrorCode::InvalidMessage,
        ),
        (
            "write",
            json!({"workspaceId":"wks_a","text":"","mode":"append"}),
            ErrorCode::InvalidMessage,
        ),
        (
            "write",
            json!({"workspaceId":"wks_a","text":"t","author":{"kind":"agent"}}),
            ErrorCode::InvalidMessage,
        ),
        (
            "write",
            json!({"workspaceId":"wks_a","text":"t","mode":"merge"}),
            ErrorCode::InvalidMessage,
        ),
    ];
    for (method, params, expected) in cases {
        assert_eq!(
            call(&fixture, method, params.clone()),
            Err(expected),
            "{method} {params}"
        );
    }
    assert_eq!(
        execute(
            &fixture.notepads,
            "workspace.notepad.list.request",
            json!({})
        ),
        Err(ErrorCode::MethodNotFound)
    );
    let _ = &fixture.workspaces;
}
