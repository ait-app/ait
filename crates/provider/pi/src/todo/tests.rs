use super::*;

#[test]
fn native_extension_details_keep_identity_and_complete_or_clear_the_snapshot() {
    assert_eq!(
        snapshot(&json!({"details":{"todos":[
            {"id":1,"text":"Build","done":false},
            {"id":2,"text":"Verify","done":true}
        ]}})),
        Some(json!({"type":"todo","items":[
            {"id":"1","text":"Build","status":"pending","completed":false},
            {"id":"2","text":"Verify","status":"completed","completed":true}
        ]}))
    );
    assert_eq!(
        snapshot(&json!({"details":{"todos":[]}})),
        Some(json!({"type":"todo","items":[]}))
    );
    for details in [
        json!({"todos":[{"id":1,"text":"Build","done":false}],"error":"failed"}),
        json!({"todos":[{"id":1,"text":"Build","done":"yes"}]}),
        json!({"todos":[{"text":"Build","done":false}]}),
        json!({"todos":vec![json!({"id":1,"text":"Build","done":false});1025]}),
        json!({"todos":[{"id":1,"text":"x".repeat(8193),"done":false}]}),
        json!(null),
    ] {
        assert!(snapshot(&json!({"details":details})).is_none());
    }
}
