use super::*;

#[tokio::test]
async fn single_mode_negotiates_all_methods_and_owns_release() {
    let fixture = Fixture::start().await;
    let info = fixture.api.shared.info();
    assert!(info.capabilities.len() > 64);
    let mut socket = fixture.socket().await;
    send(
        &mut socket,
        json!({"type":"hello","client_id":"single-client",
        "protocol":{"major":1,"min_minor":0,"max_minor":0},
        "capabilities":info.capabilities,"required_capabilities":["connection.single.v1"]}),
    )
    .await;
    let welcome = receive(&mut socket).await;
    assert_eq!(welcome["type"], "server_info");
    assert!(welcome["negotiated_capabilities"].as_array().unwrap().len() > 64);
    assert_eq!(
        request(&mut socket, "connection.ping", json!({"nonce":"alive"})).await["result"]["nonce"],
        "alive"
    );
    let subscribed = request(&mut socket, "server.status.subscribe", json!({})).await;
    let id = subscribed["result"]["subscription_id"].clone();
    // A status update can precede the release response.
    send(
        &mut socket,
        json!({"type":"request","request_id":"release","method":"subscription.release.request",
        "params":{"subscriptionId":id}}),
    )
    .await;
    loop {
        let response = receive(&mut socket).await;
        if response["type"] == "event" || response["type"] == "status" {
            continue;
        }
        assert_eq!(response["request_id"], "release");
        assert_eq!(response["result"]["subscriptionId"], id);
        break;
    }
    socket.close(None).await.unwrap();
    fixture.stop().await;
}

#[tokio::test]
async fn single_mode_still_rejects_unbounded_capability_offers() {
    let fixture = Fixture::start().await;
    let mut socket = fixture.socket().await;
    send(&mut socket, json!({"type":"hello","client_id":"single-client",
        "protocol":{"major":1,"min_minor":0,"max_minor":0},
        "capabilities":vec!["connection.ping";257],"required_capabilities":["connection.single.v1"]})).await;
    assert_eq!(receive(&mut socket).await["code"], "invalid_message");
    drop(socket);
    fixture.stop().await;
}
