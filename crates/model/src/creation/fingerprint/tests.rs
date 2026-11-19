use super::of;
use serde_json::{Value, json};

#[test]
fn version_one_matches_the_stored_digest() {
    let value = json!({"a":[false,null,1], "b":-0.0, "c":-4, "d":"载"});
    assert_eq!(
        of(&value),
        [
            0xff, 0x5c, 0x0f, 0x1a, 0x22, 0x99, 0x3c, 0x50, 0x95, 0xf0, 0xba, 0x33, 0xc2, 0xe6,
            0x7a, 0x7d, 0xc0, 0xf5, 0x7b, 0x07, 0x52, 0xe4, 0x6f, 0x4b, 0x74, 0xa3, 0x26, 0x4f,
            0x95, 0x23, 0xe4, 0x41,
        ]
    );
}

#[test]
fn fingerprints_preserve_json_equality_and_all_request_fields() {
    let left: Value = serde_json::from_str(r#"{"b":-0.0,"a":[false,null,1]}"#).unwrap();
    assert_eq!(of(&left), of(&json!({"a":[false,null,1],"b":0.0})));
    for changed in [
        json!({"a":[null,false,1],"b":0.0}),
        json!({"a":[false,null,1.0],"b":0.0}),
        json!({"a":[false,null,"1"],"b":0.0}),
        json!({"a":[false,null,1],"b":0.0,"images":[]}),
    ] {
        assert_ne!(of(&left), of(&changed));
    }
}
