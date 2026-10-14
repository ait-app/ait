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

#[tokio::test]
async fn single_mode_correlates_negotiation_and_release_errors_without_disconnect() {
    let fixture = Fixture::start().await;
    let mut socket = fixture.socket().await;
    send(
        &mut socket,
        json!({"type":"hello","client_id":"single-client",
        "protocol":{"major":1,"min_minor":0,"max_minor":0},
        "capabilities":["connection.single.v1","connection.ping","subscription.release.request"],
        "required_capabilities":["connection.single.v1"]}),
    )
    .await;
    assert_eq!(receive(&mut socket).await["type"], "server_info");
    for (method, params, code) in [
        ("missing.request", json!({}), "method_not_found"),
        ("server.info", json!({}), "unsupported_capability"),
        ("terminal.input", json!({}), "invalid_message"),
        ("subscription.release.request", json!({}), "invalid_message"),
        (
            "subscription.release.request",
            json!({"subscriptionId":""}),
            "invalid_message",
        ),
    ] {
        let response = request(&mut socket, method, params).await;
        assert_eq!(response["request_id"], "r1");
        assert_eq!(response["code"], code, "{method}: {response}");
    }
    assert_eq!(
        request(
            &mut socket,
            "connection.ping",
            json!({"nonce":"still-alive"})
        )
        .await["result"]["nonce"],
        "still-alive"
    );
    socket.close(None).await.unwrap();
    fixture.stop().await;
}

#[tokio::test]
async fn single_mode_rejects_an_invalid_request_identity_and_closes_only_its_socket() {
    let fixture = Fixture::start().await;
    let mut socket = fixture.socket().await;
    let mut offer = hello();
    offer["required_capabilities"] = json!(["connection.single.v1"]);
    send(&mut socket, offer).await;
    assert_eq!(receive(&mut socket).await["type"], "server_info");
    send(
        &mut socket,
        json!({"type":"request","request_id":"",
        "method":"connection.ping","params":{}}),
    )
    .await;
    assert_eq!(receive(&mut socket).await["code"], "invalid_message");
    drop(socket);
    let mut sibling = fixture.socket().await;
    send(&mut sibling, hello()).await;
    assert_eq!(receive(&mut sibling).await["type"], "server_info");
    assert_eq!(
        request(&mut sibling, "connection.ping", json!({"nonce":"sibling"})).await["result"]["nonce"],
        "sibling"
    );
    sibling.close(None).await.unwrap();
    fixture.stop().await;
}
