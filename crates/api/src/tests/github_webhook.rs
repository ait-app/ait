use super::*;

#[tokio::test]
async fn github_webhook_uses_platform_signature_instead_of_daemon_bearer_token() {
    let fixture = Fixture::start().await;
    let channel = domain::ChannelId::generate();
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(fixture.url(&format!("/v1/bots/github/{channel}/webhook")))
        .header("x-github-event", "ping")
        .header("x-github-delivery", "delivery-1")
        .header("x-hub-signature-256", format!("sha256={}", "0".repeat(64)))
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let protected = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(fixture.url("/v1/bots/github/not-a-channel/webhook"))
        .send()
        .await
        .unwrap();
    assert_eq!(protected.status(), StatusCode::UNAUTHORIZED);
    fixture.stop().await;
}
