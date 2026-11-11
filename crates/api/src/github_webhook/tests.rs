use axum::http::Method;

use super::is_request;

#[test]
fn recognizes_only_post_with_a_valid_channel_identity() {
    let path = "/v1/bots/github/8dc1e910-e189-4d42-bc5a-018bcd1f876b/webhook";
    assert!(is_request(&Method::POST, path));
    assert!(!is_request(&Method::GET, path));
    assert!(!is_request(
        &Method::POST,
        "/v1/bots/github/not-a-channel/webhook"
    ));
    assert!(!is_request(
        &Method::POST,
        "/v1/bots/github/8dc1e910-e189-4d42-bc5a-018bcd1f876b/other"
    ));
}
