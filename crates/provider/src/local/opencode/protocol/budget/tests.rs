use super::*;
use serde_json::json;

#[test]
fn limits_ignore_prepared_history_and_reject_new_tokens_items_or_text() {
    let prepared: Snapshot = serde_json::from_value(
        json!({"driver":"opencode","id":"ses_one","input_id":"input",
        "cwd":"/tmp","model":"local/model","reasoning_effort":null,"outcome":null,"messages":[]}),
    )
    .unwrap();
    let raw = [
        json!({"id":"a1","type":"assistant","content":[{"type":"text","text":"answer"}],
        "tokens":{"input":4,"output":3,"reasoning":2}}),
    ];
    for limits in [
        OpenCodeExecutionLimits {
            max_tokens: 8,
            ..Default::default()
        },
        OpenCodeExecutionLimits {
            max_output_bytes: 5,
            ..Default::default()
        },
    ] {
        assert_eq!(
            validate(Version::V2, &raw, &prepared, limits)
                .unwrap_err()
                .code,
            Fault::RunLimitExceeded
        );
    }
    let mut many = raw.to_vec();
    many[0]["content"] = json!([{"type":"text","text":"a"},{"type":"text","text":"b"}]);
    assert!(
        validate(
            Version::V2,
            &many,
            &prepared,
            OpenCodeExecutionLimits {
                max_steps: 1,
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(
        validate(
            Version::V2,
            &raw,
            &prepared,
            OpenCodeExecutionLimits::default()
        )
        .is_ok()
    );
    assert!(
        crate::local::opencode::Driver::new("opencode".into())
            .with_execution_limits(OpenCodeExecutionLimits::default())
            .is_ok()
    );
    assert!(
        crate::local::opencode::Driver::new("opencode".into())
            .with_execution_limits(OpenCodeExecutionLimits {
                max_steps: 0,
                ..Default::default()
            })
            .is_err()
    );
}
