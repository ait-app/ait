use super::*;

#[test]
fn ordinary_structured_fields_and_existing_large_outputs_are_preserved() {
    for value in [
        Value::Null,
        json!(42),
        json!([true, {"text":"你好"}]),
        json!({"content":[{"type":"text","text":"Success"}],
            "structuredContent":{"log":"x".repeat(350 * 1024)},"isError":false}),
    ] {
        assert_eq!(bounded(&value, OUTPUT_BYTES), value);
    }
    let native = json!({"arguments":{"url":"https://example.test"},"result":{"ok":true}});
    assert_eq!(
        detail(&native),
        json!({"type":"unknown",
        "input":native["arguments"],"output":native["result"]})
    );
    assert_eq!(
        error(&json!({"message":"Access denied"})),
        json!({"message":"Access denied"})
    );
}

#[test]
fn exact_json_budget_is_preserved_and_one_extra_byte_produces_a_marked_preview() {
    let exact = json!("x".repeat(INPUT_BYTES - 2));
    assert_eq!(bounded(&exact, INPUT_BYTES), exact);
    let excessive = json!("x".repeat(INPUT_BYTES - 1));
    let preview = bounded(&excessive, INPUT_BYTES);
    assert!(preview.as_str().unwrap().ends_with(MARKER));
    assert!(serde_json::to_vec(&preview).unwrap().len() <= INPUT_BYTES);
}

#[test]
fn utf8_and_json_escaping_remain_bounded_for_arguments_results_and_errors() {
    for text in ["界", "\0\n\"\\"] {
        let value = json!({"text":text.repeat(OUTPUT_BYTES)});
        let native = json!({"arguments":value,"result":value});
        let detail = detail(&native);
        for (field, limit) in [("input", INPUT_BYTES), ("output", OUTPUT_BYTES)] {
            let preview = &detail[field];
            let text = preview.as_str().unwrap();
            assert!(text.ends_with(MARKER));
            assert!(!text.contains('\u{fffd}'));
            assert!(serde_json::to_vec(preview).unwrap().len() <= limit);
        }
        let error = error(&value);
        assert!(error.as_str().unwrap().ends_with(MARKER));
        assert!(serde_json::to_vec(&error).unwrap().len() <= INPUT_BYTES);
        assert_eq!(detail, super::detail(&native));
    }
}

#[test]
fn serialization_stops_after_the_budget_and_retains_only_the_prefix() {
    let mut preview = JsonPreview {
        bytes: Vec::new(),
        size: 0,
        limit: 10,
        prefix_limit: 3,
    };
    assert_eq!(preview.write(b"abcdef").unwrap(), 6);
    assert_eq!(preview.bytes, b"abc");
    assert_eq!(
        preview.write(b"ghijkl").unwrap_err().kind(),
        io::ErrorKind::WriteZero
    );
    assert_eq!(preview.bytes, b"abc");
    assert_eq!(preview.size, 12);
    preview.flush().unwrap();
}
