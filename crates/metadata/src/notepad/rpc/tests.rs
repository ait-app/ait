use serde_json::{Value, json};

use super::{PREVIEW_CHARS, execute};
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
fn write_then_list_and_get_report_revisions_and_authors() {
    let fixture = fixture();
    let written = call(
        &fixture,
        "write",
        json!({"workspaceId":"wks_a","key":"api","text":"POST /x","author":agent()}),
    )
    .unwrap();
    assert_eq!(
        written,
        json!({"accepted":true,"workspaceId":"wks_a","key":"api","revision":1,"notepadRevision":1})
    );
    call(
        &fixture,
        "write",
        json!({"workspaceId":"wks_a","key":"todo","text":"x".repeat(400)}),
    )
    .unwrap();

    let listed = call(&fixture, "list", json!({"workspaceId":"wks_a"})).unwrap();
    assert_eq!(listed["revision"], 2);
    assert_eq!(listed["total"], 2);
    assert_eq!(listed["limits"], json!({"notes":64,"textBytes":16384}));
    assert_eq!(listed["notes"][0]["key"], "api");
    assert_eq!(listed["notes"][0]["createdBy"], agent());
    assert_eq!(listed["notes"][0]["preview"], "POST /x");
    assert_eq!(listed["notes"][1]["updatedBy"], json!({"kind":"user"}));
    assert_eq!(listed["notes"][1]["chars"], 400);
    assert_eq!(
        listed["notes"][1]["preview"].as_str().unwrap().len(),
        PREVIEW_CHARS
    );
    assert!(listed["notes"][0].get("text").is_none());

    let since = call(&fixture, "list", json!({"workspaceId":"wks_a","since":1})).unwrap();
    assert_eq!(since["notes"].as_array().unwrap().len(), 1);
    assert_eq!(since["notes"][0]["key"], "todo");
    assert_eq!(since["total"], 2);

    let note = call(&fixture, "get", json!({"workspaceId":"wks_a","key":"api"})).unwrap();
    assert_eq!(note["note"]["text"], "POST /x");
    assert_eq!(note["revision"], 2);
    let absent = call(&fixture, "get", json!({"workspaceId":"wks_a","key":"none"})).unwrap();
    assert_eq!(absent["note"], Value::Null);
}

#[test]
fn previews_end_on_a_character_boundary() {
    let fixture = fixture();
    let text = "é".repeat(PREVIEW_CHARS + 5);
    call(
        &fixture,
        "write",
        json!({"workspaceId":"wks_a","key":"k","text":text}),
    )
    .unwrap();
    let listed = call(&fixture, "list", json!({"workspaceId":"wks_a"})).unwrap();
    assert_eq!(
        listed["notes"][0]["preview"]
            .as_str()
            .unwrap()
            .chars()
            .count(),
        PREVIEW_CHARS
    );
}

#[test]
fn refusals_are_responses_that_carry_what_a_retry_needs() {
    let fixture = fixture();
    let base = json!({"workspaceId":"wks_a","key":"api","text":"one","author":agent()});
    call(&fixture, "write", base.clone()).unwrap();

    let exists = call(&fixture, "write", base.clone()).unwrap();
    assert_eq!(exists["accepted"], false);
    assert_eq!(exists["revision"], 1);
    assert_eq!(exists["updatedBy"], agent());
    assert!(exists["error"].as_str().unwrap().contains("already exists"));
    assert!(exists.get("notepadRevision").is_none());

    let mut stale = base.clone();
    stale["expected"] = json!(9);
    let conflict = call(&fixture, "write", stale).unwrap();
    assert_eq!(
        (&conflict["accepted"], &conflict["revision"]),
        (&json!(false), &json!(1))
    );

    let mut append = base;
    append["mode"] = json!("append");
    let appended = call(&fixture, "write", append).unwrap();
    assert_eq!(
        (&appended["accepted"], &appended["revision"]),
        (&json!(true), &json!(2))
    );

    let missing = call(
        &fixture,
        "delete",
        json!({"workspaceId":"wks_a","key":"none","expected":1}),
    )
    .unwrap();
    assert_eq!(missing["accepted"], false);
    assert!(missing.get("revision").is_none());
    let stale = call(
        &fixture,
        "delete",
        json!({"workspaceId":"wks_a","key":"api","expected":1}),
    )
    .unwrap();
    assert_eq!(
        (&stale["accepted"], &stale["revision"]),
        (&json!(false), &json!(2))
    );
    let deleted = call(
        &fixture,
        "delete",
        json!({"workspaceId":"wks_a","key":"api","expected":2}),
    )
    .unwrap();
    assert_eq!(
        deleted,
        json!({"accepted":true,"workspaceId":"wks_a","key":"api","notepadRevision":3})
    );
}

#[test]
fn invalid_requests_and_unknown_workspaces_are_errors() {
    let fixture = fixture();
    let cases = [
        (
            "list",
            json!({"workspaceId":"wks_missing"}),
            ErrorCode::WorkspaceNotFound,
        ),
        (
            "get",
            json!({"workspaceId":"wks_missing","key":"k"}),
            ErrorCode::WorkspaceNotFound,
        ),
        (
            "list",
            json!({"workspaceId":"wks_a","extra":1}),
            ErrorCode::InvalidMessage,
        ),
        (
            "write",
            json!({"workspaceId":"wks_a","key":"Bad","text":"t"}),
            ErrorCode::InvalidMessage,
        ),
        (
            "write",
            json!({"workspaceId":"wks_a","key":"k","text":""}),
            ErrorCode::InvalidMessage,
        ),
        (
            "write",
            json!({"workspaceId":"wks_a","key":"k","text":"t","author":{"kind":"agent"}}),
            ErrorCode::InvalidMessage,
        ),
        (
            "write",
            json!({"workspaceId":"wks_a","key":"k","text":"t","mode":"merge"}),
            ErrorCode::InvalidMessage,
        ),
        (
            "delete",
            json!({"workspaceId":"wks_a","key":"k"}),
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
            "workspace.notepad.rename.request",
            json!({})
        ),
        Err(ErrorCode::MethodNotFound)
    );
}

#[test]
fn full_and_oversized_notepads_refuse_with_guidance() {
    let fixture = fixture();
    let large = "x".repeat(16 * 1024 + 1);
    let oversized = call(
        &fixture,
        "write",
        json!({"workspaceId":"wks_a","key":"k","text":large}),
    )
    .unwrap();
    assert_eq!(oversized["accepted"], false);
    assert!(oversized["error"].as_str().unwrap().contains("16 KiB"));
    for index in 0..64 {
        let params = json!({"workspaceId":"wks_a","key":format!("note-{index}"),"text":"t"});
        assert_eq!(call(&fixture, "write", params).unwrap()["accepted"], true);
    }
    let full = call(
        &fixture,
        "write",
        json!({"workspaceId":"wks_a","key":"more","text":"t"}),
    )
    .unwrap();
    assert_eq!(full["accepted"], false);
    assert!(full["error"].as_str().unwrap().contains("full"));
    let _ = &fixture.workspaces;
    let _ = &fixture.store;
}
