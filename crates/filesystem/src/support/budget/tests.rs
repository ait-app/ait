use super::{OUTPUT_BUDGET, encoded_size, fits};

#[test]
fn encoded_sizes_count_utf8_and_json_escaping_without_a_budget_limit() {
    for value in ["你好\"\\\t\0".to_owned(), "x".repeat(OUTPUT_BUDGET)] {
        let actual = serde_json::to_vec(&value).unwrap();
        assert_eq!(encoded_size(&value).unwrap(), actual.len());
        assert_eq!(fits(&value), actual.len() <= OUTPUT_BUDGET);
    }
}

#[test]
fn encoded_budget_accepts_its_exact_limit() {
    assert!(fits(&"x".repeat(OUTPUT_BUDGET - 2)));
    assert!(!fits(&"x".repeat(OUTPUT_BUDGET - 1)));
}
