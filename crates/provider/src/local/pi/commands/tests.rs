use super::*;

#[test]
fn discovery_maps_unsupported_requests_and_validates_catalogs() {
    assert_eq!(
        finish_discovery(Err(AgentSessionError::Rejected)),
        Err(AgentSessionError::Unavailable)
    );
    assert_eq!(
        finish_discovery(Err(AgentSessionError::Busy)),
        Err(AgentSessionError::Busy)
    );
    assert_eq!(finish_discovery(Ok(json!({"commands":[]}))), Ok(vec![]));
    assert_eq!(
        finish_discovery(Ok(json!({}))),
        Err(AgentSessionError::Failed)
    );
}

use std::io::Write;

#[test]
fn catalog_budget_accepts_exact_limit_and_rejects_overflow() {
    let mut budget = CatalogBudget(3);
    assert_eq!(budget.write(b"ab").unwrap(), 2);
    assert_eq!(budget.write(b"c").unwrap(), 1);
    assert!(budget.write(b"d").is_err());
    budget.flush().unwrap();
}

#[test]
fn catalog_size_counts_json_escaping_and_array_delimiters() {
    let limit = 768 * 1024;
    assert!(catalog_fits(&[json!("x".repeat(limit - 4))]));
    assert!(!catalog_fits(&[json!("x".repeat(limit - 3))]));
    assert!(!catalog_fits(&[json!("\\".repeat(limit / 2))]));
}

#[test]
fn command_labels_preserve_native_hints_with_bounded_sanitization() {
    for (hint, expected) in [
        (json!("<topic>"), "<topic>".to_owned()),
        (Value::Null, String::new()),
        (json!(42), String::new()),
        (json!("a\nb\t"), "a b ".to_owned()),
        (json!("é".repeat(513)), "é".repeat(512)),
    ] {
        let catalog =
            catalog(&json!({"commands":[{"name":"review", "argumentHint":hint}]})).unwrap();
        assert_eq!(catalog[0]["argumentHint"], expected);
    }
    let catalog = catalog(&json!({"commands":[{"name":"review"}]})).unwrap();
    assert_eq!(catalog[0]["argumentHint"], "");
}
