use super::super::streaming::preview_with_size;
use super::*;

#[test]
fn previews_preserve_small_values_and_truncate_unicode_without_quotes() {
    for value in [Value::Null, json!({"output":"short"}), json!("short")] {
        assert_eq!(preview_with_size(&value).0, value);
    }
    let text = format!("{}{}", "a".repeat(32 * 1024 - 1), "界".repeat(100));
    let result = preview_with_size(&json!(text)).0;
    let result = result.as_str().unwrap();
    assert!(result.starts_with(&"a".repeat(32 * 1024 - 1)));
    assert!(!result.contains('界'));
    assert!(result.ends_with("[Output truncated; full output remains in the native transcript.]"));
    assert!(
        preview_with_size(&json!({"output":"x".repeat(40*1024)}))
            .0
            .is_string()
    );
}

#[test]
fn preview_sizes_measure_display_payload_without_reserializing_strings() {
    for value in [
        json!("é\n\""),
        json!({"nested":[1,true,null]}),
        json!("界".repeat(20_000)),
    ] {
        let (preview, bytes) = preview_with_size(&value);
        assert_eq!(
            bytes,
            preview
                .as_str()
                .map_or_else(|| preview.to_string().len(), str::len)
        );
    }
}
