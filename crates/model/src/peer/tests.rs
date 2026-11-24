use super::*;

#[test]
fn a_rejected_call_shows_the_targets_code_and_message() {
    let error = Error::Rejected {
        code: -32_602,
        message: "workspaceId is required".to_owned(),
    };
    assert_eq!(
        error.to_string(),
        "host rejected the call (-32602): workspaceId is required"
    );
}
