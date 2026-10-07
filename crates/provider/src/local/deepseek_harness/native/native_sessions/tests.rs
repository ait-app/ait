use super::*;

#[test]
fn list_filters_before_limiting_and_keeps_native_title_and_activity_order() {
    let response = json!({"items":[
        {"sessionId":"blank","cwd":"/tmp","blank":true,"updatedAt":5},
        {"sessionId":"child","cwd":"/tmp","origin":"subagent","updatedAt":5},
        {"sessionId":"foreign","cwd":"/elsewhere","updatedAt":5},
        {"sessionId":"no-directory","updatedAt":5},
        {"sessionId":"older","cwd":"/tmp","updatedAt":1},
        {"sessionId":"newer","cwd":"/tmp","updatedAt":2,"projections":{"values":{"title":"Native title"}}}
    ]});
    let entries = descriptors(
        &response,
        &ListOptions {
            cwd: Some("/tmp".into()),
            scan_limit: 1,
        },
    )
    .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].provider_handle_id, "newer");
    assert_eq!(entries[0].title.as_deref(), Some("Native title"));
    assert_eq!(entries[0].last_activity_at, "1970-01-01T00:00:00.002+00:00");
    assert_eq!(
        descriptors(
            &response,
            &ListOptions {
                cwd: None,
                scan_limit: 20
            }
        )
        .unwrap()
        .len(),
        3
    );
}

#[test]
fn malformed_list_records_and_identifiers_fail_without_fabricated_metadata() {
    for response in [
        json!({}),
        json!({"items":[{"sessionId":"id","cwd":"relative","updatedAt":0}]}),
        json!({"items":[{"sessionId":"id","cwd":"/tmp"}]}),
    ] {
        assert!(
            descriptors(
                &response,
                &ListOptions {
                    cwd: None,
                    scan_limit: 20
                }
            )
            .is_err()
        );
    }
    for id in ["", "invalid\nidentity", &"x".repeat(1025)] {
        assert!(validate_id(id).is_err());
    }
    assert!(timestamp(i64::MAX).is_err());
}
