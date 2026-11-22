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
fn a_notepad_reads_empty_until_written_and_then_reports_its_writer() {
    let fixture = fixture();
    assert_eq!(
        call(&fixture, "get", json!({"workspaceId":"wks_a"})).unwrap(),
        json!({"workspaceId":"wks_a","revision":0,"text":""})
    );

    let written = call(
        &fixture,
        "write",
        json!({"workspaceId":"wks_a","text":"POST /x","author":agent()}),
    )
    .unwrap();
    assert_eq!(written["accepted"], true);
    assert_eq!(written["revision"], 1);
    assert_eq!(written["updatedBy"], agent());
    assert!(written.get("error").is_none());

    let read = call(&fixture, "get", json!({"workspaceId":"wks_a"})).unwrap();
    assert_eq!(
        (&read["text"], &read["revision"]),
        (&json!("POST /x"), &json!(1))
    );
    assert_eq!(read["updatedBy"], agent());
    assert_eq!(read["updatedAt"], written["updatedAt"]);
    assert_eq!(
        call(&fixture, "get", json!({"workspaceId":"wks_b"})).unwrap()["revision"],
        0
    );
}

#[test]
fn a_client_write_is_a_users_and_a_stale_one_is_refused_with_the_current_state() {
    let fixture = fixture();
    let first = call(
        &fixture,
        "write",
        json!({"workspaceId":"wks_a","text":"one","expected":0}),
    )
    .unwrap();
    assert_eq!(first["updatedBy"], json!({"kind":"user"}));

    for stale in [json!({}), json!({"expected":0}), json!({"expected":9})] {
        let mut params = json!({"workspaceId":"wks_a","text":"two","author":agent()});
        params
            .as_object_mut()
            .unwrap()
            .extend(stale.as_object().unwrap().clone());
        let refused = call(&fixture, "write", params).unwrap();
        assert_eq!(
            (&refused["accepted"], &refused["revision"]),
            (&json!(false), &json!(1))
        );
        assert_eq!(refused["updatedBy"], json!({"kind":"user"}));
        assert!(refused["error"].as_str().unwrap().contains("read it again"));
    }

    let appended = call(
        &fixture,
        "write",
        json!({"workspaceId":"wks_a","text":"two","mode":"append","author":agent()}),
    )
    .unwrap();
    assert_eq!(
        (&appended["accepted"], &appended["revision"]),
        (&json!(true), &json!(2))
    );
    let cleared = call(
        &fixture,
        "write",
        json!({"workspaceId":"wks_a","text":"","expected":2}),
    )
    .unwrap();
    assert_eq!(
        (&cleared["accepted"], &cleared["revision"]),
        (&json!(true), &json!(3))
    );
    assert_eq!(
        call(&fixture, "get", json!({"workspaceId":"wks_a"})).unwrap()["text"],
        ""
    );
}

#[test]
fn an_oversized_notepad_refuses_with_guidance() {
    let fixture = fixture();
    let large = "x".repeat(256 * 1024 + 1);
    let refused = call(
        &fixture,
        "write",
        json!({"workspaceId":"wks_a","text":large}),
    )
    .unwrap();
    assert_eq!(
        (&refused["accepted"], &refused["revision"]),
        (&json!(false), &json!(0))
    );
    assert!(refused["error"].as_str().unwrap().contains("256 KiB"));
    assert!(refused.get("updatedBy").is_none());
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
    let _ = (&fixture.workspaces, &fixture.store);
}
