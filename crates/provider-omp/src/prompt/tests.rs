use super::*;

fn blocks() -> Vec<Value> {
    vec![
        json!({"type":"text","text":"look"}),
        json!({"type":"image","data":"aW1hZ2U=","mimeType":"image/png"}),
        json!({"type":"text","text":"then answer"}),
    ]
}

#[test]
fn images_reach_omp_in_order_beside_the_joined_text() {
    let payload = Payload::new(blocks());

    assert_eq!(payload.text, "look\n\nthen answer");
    assert_eq!(
        payload.fields(Some("steer")),
        json!({"message":"look\n\nthen answer",
            "images":[{"type":"image","data":"aW1hZ2U=","mimeType":"image/png"}],
            "streamingBehavior":"steer"})
    );
}

#[test]
fn an_input_without_images_omits_the_key() {
    let payload = Payload::new(vec![
        json!({"type":"text","text":"only"}),
        json!({"type":"text","text":"text"}),
    ]);

    // As Paseo does, an input without images omits the key.
    assert_eq!(payload.fields(None), json!({"message":"only\n\ntext"}));
}
