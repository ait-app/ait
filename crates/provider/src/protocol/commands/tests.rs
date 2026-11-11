use super::*;

fn result() -> CompletionResult {
    serde_json::from_value(serde_json::json!({
        "supported": true,
        "items": [{"id":"nested", "label":"on", "insertText":"on ",
            "replaceRange":{"start":7,"end":8}, "cursor":10,
            "triggerCompletion":true}]
    }))
    .expect("completion fixture")
}

#[test]
fn validates_native_nested_edit_and_explicit_caret() {
    let input = CompletionInput {
        text: "/review o suffix".into(),
        cursor: 9,
    };
    assert!(result().validate(&input).is_ok());
}

#[test]
fn rejects_surrogate_boundaries_and_oversized_input() {
    let mut input = CompletionInput {
        text: "/cmd 😀".into(),
        cursor: 6,
    };
    assert!(input.validate().is_err());
    input.cursor = 7;
    assert!(input.validate().is_ok());
    input.text = "a".repeat(MAX_TEXT + 1);
    assert!(input.validate().is_err());
    input.text = "\0".into();
    assert!(input.validate().is_err());
}

#[test]
fn rejects_invalid_ranges_carets_duplicates_and_unsupported_candidates() {
    let input = CompletionInput {
        text: "/review o suffix".into(),
        cursor: 9,
    };
    let mut invalid = result();
    invalid.items[0].replace_range.end = 99;
    assert!(invalid.validate(&input).is_err());
    invalid = result();
    invalid.items[0].cursor = Some(99);
    assert!(invalid.validate(&input).is_err());
    invalid = result();
    invalid.items.push(invalid.items[0].clone());
    assert!(invalid.validate(&input).is_err());
    invalid = result();
    invalid.supported = false;
    assert!(invalid.validate(&input).is_err());
    assert!(CompletionResult::default().validate(&input).is_ok());
}

#[test]
fn rejects_oversized_final_text_without_an_explicit_caret() {
    let input = CompletionInput {
        text: "a".repeat(MAX_TEXT),
        cursor: 0,
    };
    let mut invalid = result();
    invalid.items[0].replace_range = CompletionRange { start: 0, end: 0 };
    invalid.items[0].cursor = None;
    assert!(invalid.validate(&input).is_err());
}

#[test]
fn reserves_response_envelope_space_when_bounding_native_candidates() {
    let input = CompletionInput {
        text: "/review o".into(),
        cursor: 9,
    };
    let mut invalid = result();
    invalid.items[0].description = Some("x".repeat(MAX_ITEMS_BYTES));
    assert!(invalid.validate(&input).is_err());
}
