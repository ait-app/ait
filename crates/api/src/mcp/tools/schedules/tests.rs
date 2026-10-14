use super::*;

#[test]
fn duration_parser_checks_the_entire_value_and_overflow() {
    for (text, milliseconds) in [
        ("30", 30_000),
        ("30s", 30_000),
        ("5m", 300_000),
        ("2h30m", 9_000_000),
    ] {
        assert_eq!(duration_ms(&json!(text)).unwrap(), milliseconds);
    }
    for text in [
        "",
        "0",
        "1m trailing",
        "30x",
        "h",
        "1h2",
        "9223372036854775807h",
    ] {
        assert!(duration_ms(&json!(text)).is_err(), "{text}");
    }
    assert!(expiry_at(&json!("5m")).unwrap().is_string());
}

#[test]
fn schedule_patch_translates_cadence_expiry_and_native_config() {
    let value = update(
        json!({"scheduleId":"s","cron":"0 * * * *","timezone":"Asia/Shanghai","clearExpires":true,
        "provider":"codex/model","mode":"full-access","cwd":"/repo"}),
    )
    .unwrap();
    assert_eq!(
        value["cadence"],
        json!({"type":"cron","expression":"0 * * * *","timezone":"Asia/Shanghai"})
    );
    assert_eq!(
        value["newAgentConfig"],
        json!({"provider":"codex","model":"model","modeId":"full-access","cwd":"/repo"})
    );
    assert_eq!(value["expiresAt"], Value::Null);
    assert_eq!(
        update(json!({"scheduleId":"s","every":"5m"})).unwrap()["cadence"],
        json!({"type":"every","everyMs":300_000})
    );
    assert!(update(json!({"scheduleId":"s","expiresIn":"5m"})).unwrap()["expiresAt"].is_string());
    for input in [
        json!({"cron":"0 * * * *","every":"5m"}),
        json!({"every":"5m","timezone":"UTC"}),
        json!({"timezone":"UTC"}),
        json!({"expiresIn":"5m","clearExpires":true}),
        json!({"provider":"codex/model","model":"other"}),
    ] {
        assert!(update(input).is_err());
    }
}
