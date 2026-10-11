use super::model;
use serde_json::json;

#[test]
fn model_keeps_native_metadata_and_only_supported_efforts() {
    let choice =
        json!({"value":"local/test","name":"Native test","description":"Native description"});
    let options = json!([{"id":"effort","category":"thought_level","type":"select",
        "currentValue":"high","options":[{"value":"high","name":"High"}]}]);

    let projected = model(&choice, &options, "local/test").unwrap();

    assert_eq!(projected["description"], "Native description");
    assert_eq!(projected["isDefault"], true);
    assert_eq!(projected["defaultThinkingOptionId"], "high");
    assert_eq!(projected["thinkingOptions"][0]["isDefault"], true);
    let plain = model(&choice, &json!([]), "local/other").unwrap();
    assert_eq!(plain["thinkingOptions"], json!([]));
    assert_eq!(plain["isDefault"], false);
    assert!(plain.get("defaultThinkingOptionId").is_none());
    assert!(
        model(
            &choice,
            &json!([{"category":"thought_level","type":"select"}]),
            "local/test"
        )
        .is_err()
    );
}
