use serde_json::json;

use super::permitted;

#[test]
fn a_peer_never_names_a_host_even_where_the_schema_offers_one() {
    let schema = json!({"type":"object","properties":{"agentId":{"type":"string"},
        "hostId":{"type":"string"}},"required":["agentId"],"additionalProperties":false});

    assert!(permitted(&schema, &json!({"agentId":"a"})));
    assert!(!permitted(&schema, &json!({"agentId":"a","hostId":"h"})));
    assert!(!permitted(&schema, &json!({"agentId":"a","other":1})));
    assert!(!permitted(&schema, &json!([])));
}
