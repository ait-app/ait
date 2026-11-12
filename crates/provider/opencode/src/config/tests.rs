use super::confirmation;
use serde_json::json;

#[test]
fn confirmation_requires_the_selected_value_and_session_and_validates_options() {
    let mut message = json!({"method":"session/update","params":{"sessionId":"session",
        "update":{"sessionUpdate":"config_option_update","configOptions":[
            {"id":"model","category":"model","type":"select","currentValue":"local/model",
                "options":[{"value":"local/model","name":"Model"}]}]}}});
    assert!(
        confirmation(&message, "session", "model", "local/model")
            .unwrap()
            .is_some()
    );
    assert!(
        confirmation(&message, "session", "model", "local/other")
            .unwrap()
            .is_none()
    );
    assert!(confirmation(&message, "wrong", "model", "local/model").is_err());
    message["params"]["update"]["configOptions"] = json!([]);
    assert!(
        confirmation(&message, "session", "model", "local/model")
            .unwrap()
            .is_none()
    );
    message["params"]["update"]["configOptions"] = json!(null);
    assert!(confirmation(&message, "session", "model", "local/model").is_err());
    assert!(
        confirmation(&json!({}), "session", "model", "local/model")
            .unwrap()
            .is_none()
    );
}
