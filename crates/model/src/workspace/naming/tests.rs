use super::*;
use serde_json::json;

#[test]
fn empty_creation_context_waits_for_a_real_prompt_or_attachment() {
    assert!(first_agent_source(None, &[]).is_none());
    assert!(first_agent_source(Some(" \n "), &[]).is_none());
    assert!(
        first_agent_source(Some("Fix titles"), &[])
            .unwrap()
            .contains("Fix titles")
    );
    assert!(
        first_agent_source(None, &[json!({"name":"review.txt"})])
            .unwrap()
            .contains("review.txt")
    );
}
