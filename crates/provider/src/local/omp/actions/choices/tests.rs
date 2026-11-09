use super::*;

#[test]
fn long_sessions_produce_parseable_recent_choices_without_message_bodies() {
    let entries: Vec<_> = (0..200)
        .map(|index| {
            json!({"id":format!("entry-{index}"),"type":"message",
            "message":{"role":"user","content":format!("Task {index} {}", "x".repeat(4000))}})
        })
        .collect();
    let text = history(&json!({"entries":entries,"leafId":"entry-199"})).unwrap();
    let result: Value = serde_json::from_str(&text).unwrap();
    let choices = result["entries"].as_array().unwrap();
    assert_eq!(choices.len(), MAX_CHOICES);
    assert_eq!(result["omitted"], 200 - MAX_CHOICES);
    assert_eq!(result["leafId"], "entry-199");
    assert_eq!(choices.last().unwrap()["id"], "entry-199");
    assert_eq!(choices[0]["role"], "user");
    assert!(choices.iter().all(|choice| choice.get("message").is_none()));
    assert!(text.len() < 32768);
}

#[test]
fn escaped_identifiers_and_multibyte_labels_obey_the_encoded_budget() {
    let entries: Vec<_> = (0..128)
        .map(
            |index| json!({"id":format!("{index}{}", "\\\"".repeat(120)),"label":"界".repeat(500)}),
        )
        .collect();
    let text = history(&json!({"entries":entries})).unwrap();
    let result: Value = serde_json::from_str(&text).unwrap();
    assert!(text.len() < 32768);
    assert!(result["omitted"].as_u64().unwrap() > 0);
    assert!(
        result["entries"][0]["label"]
            .as_str()
            .unwrap()
            .chars()
            .count()
            <= 120
    );
}

#[test]
fn choices_keep_identity_and_role_but_not_hidden_native_state() {
    let data = json!({"entries":[
        {"id":"request","type":"message","message":{"role":"user","content":[
            {"type":"thinking","thinking":"private"},{"type":"text","text":"Build the board"}]}},
        {"id":"tool","type":"message","message":{"role":"toolResult","toolName":"read",
            "content":"very long output"}},
        {"id":"label","type":"label","label":"Checkpoint"}
    ]});
    let result: Value = serde_json::from_str(&history(&data).unwrap()).unwrap();
    assert_eq!(result["entries"][0]["label"], "Build the board");
    assert_eq!(result["entries"][1]["label"], "read");
    assert_eq!(result["entries"][2]["label"], "Checkpoint");
    assert!(!result.to_string().contains("private"));
    assert_eq!(result["omitted"], 0);
    assert!(history(&json!({})).is_none());
}
