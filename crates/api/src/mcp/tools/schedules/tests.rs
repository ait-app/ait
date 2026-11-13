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

#[test]
fn schedule_model_patch_distinguishes_omission_replacement_and_clear() {
    let omitted = update(json!({"scheduleId":"s","prompt":"changed"})).unwrap();
    assert!(omitted.get("newAgentConfig").is_none());

    let provider_only = update(json!({"scheduleId":"s","provider":"codex"})).unwrap();
    assert_eq!(provider_only["newAgentConfig"], json!({"provider":"codex"}));

    for model in [json!("namespace/model"), Value::Null] {
        let patched = update(json!({"scheduleId":"s","model":model})).unwrap();
        assert_eq!(patched["newAgentConfig"], json!({"model":model}));

        let patched = update(json!({"scheduleId":"s","provider":"codex","model":model})).unwrap();
        assert_eq!(
            patched["newAgentConfig"],
            json!({"provider":"codex","model":model})
        );
    }
}

#[test]
fn schedule_combined_selection_rejects_conflicting_model_patches() {
    for selection in [
        json!({"provider":"codex/namespace/model"}),
        json!({"provider":"codex/namespace/model","model":"namespace/model"}),
    ] {
        let patched = update(selection).unwrap();
        assert_eq!(
            patched["newAgentConfig"],
            json!({"provider":"codex","model":"namespace/model"})
        );
    }
    for model in [Value::Null, json!("other-model")] {
        assert_eq!(
            update(json!({"provider":"codex/namespace/model","model":model})),
            Err(ErrorCode::InvalidMessage)
        );
    }
}

#[test]
fn schedule_model_patch_trims_separate_and_combined_models() {
    for selection in [
        json!({"model":" namespace/model "}),
        json!({"provider":" codex ","model":" namespace/model "}),
        json!({"provider":" codex/namespace/model "}),
        json!({"provider":"codex/namespace/model","model":" namespace/model "}),
    ] {
        let patched = update(selection).unwrap();
        assert_eq!(patched["newAgentConfig"]["model"], "namespace/model");
    }
}

#[test]
fn schedule_model_patch_rejects_blank_and_non_text_models() {
    for model in [json!(""), json!(" \t\n"), json!(42), json!(false)] {
        for provider in [None, Some("codex"), Some("codex/model")] {
            let mut arguments = json!({"scheduleId":"s","model":model});
            if let Some(provider) = provider {
                arguments["provider"] = json!(provider);
            }
            assert_eq!(update(arguments), Err(ErrorCode::InvalidMessage));
        }
    }
}
