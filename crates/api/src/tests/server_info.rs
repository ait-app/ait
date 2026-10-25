use super::*;

#[tokio::test]
async fn software_version_is_present_in_http_handshake_rpc_and_lifecycle_events() {
    let fixture = Fixture::start().await;
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let http_info: Value = client
        .get(fixture.url("/v1/server/info"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(http_info["version"], model::BUILD_VERSION);

    let mut socket = fixture.socket().await;
    let mut offer = hello();
    offer["capabilities"] = json!(["server.info", "session.events.set_subscription.request"]);
    send(&mut socket, offer).await;
    let handshake = receive(&mut socket).await;
    assert_eq!(handshake["info"], http_info);

    let response = request(&mut socket, "server.info", Value::Null).await;
    assert_eq!(response["result"], http_info);
    let subscribed = request(
        &mut socket,
        "session.events.set_subscription.request",
        json!({"events":["status.server_info"]}),
    )
    .await;
    assert_eq!(subscribed["type"], "response");

    fixture.api.begin_shutdown();
    let event = receive(&mut socket).await;
    assert_eq!(event["method"], "status.server_info");
    assert_eq!(event["params"]["info"]["version"], http_info["version"]);
    assert_eq!(event["params"]["info"]["lifecycle"], "draining");
    fixture.stop().await;
}
