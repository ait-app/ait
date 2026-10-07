use super::*;

#[test]
fn legacy_editors_return_desktop_migration_without_opening_paths() {
    assert_eq!(
        execute(METHODS[0].name, json!({})).unwrap(),
        json!({"editors":[],"error":MOVED})
    );
    assert_eq!(
        execute(
            METHODS[1].name,
            json!({"path":"/missing","editorId":"code","mode":"reveal"})
        )
        .unwrap(),
        json!({"error":MOVED})
    );
    for params in [
        json!({}),
        json!({"path":"/x","editorId":" "}),
        json!({"path":"/x","editorId":"code","mode":"execute"}),
    ] {
        assert_eq!(
            execute(METHODS[1].name, params),
            Err(ErrorCode::InvalidMessage)
        );
    }
    assert_eq!(
        execute(METHODS[0].name, Value::Null),
        Err(ErrorCode::InvalidMessage)
    );
    assert_eq!(
        execute("unknown", json!({})),
        Err(ErrorCode::MethodNotFound)
    );
}
