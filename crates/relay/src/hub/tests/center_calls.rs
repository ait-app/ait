use model::hub::{CallError, Center as _, Scope};

use super::*;

async fn ready(hub: &Connector, listener: &TcpListener) -> Center {
    let (mut socket, _) = accept(listener).await;
    next(&mut socket).await;
    let mut header = welcome(Uuid::new_v4(), false, 0);
    header["center_methods"] = json!(["hub.events.publish", "hub.directory.publish"]);
    socket.send(frame(&header, b"")).await.unwrap();
    next_status(hub, |status| status.online).await;
    socket
}

fn scope(hub: &Connector) -> Scope {
    hub.state().borrow().scope.clone().unwrap()
}

#[tokio::test]
async fn center_calls_and_opaque_business_sessions_share_one_sequence_space() {
    let (_relay, hub, listener, local) = connector().await;
    let daemon = tokio::spawn(daemon(local));
    let mut socket = ready(&hub, &listener).await;
    let session = Uuid::new_v4();
    socket
        .send(open(1, session, HELLO.as_bytes()))
        .await
        .unwrap();
    assert_eq!(next(&mut socket).await.0["seq"], 1);
    assert_eq!(next(&mut socket).await.0["seq"], 2);
    let caller = hub.clone();
    let authority = scope(&hub);
    let task = tokio::spawn(async move {
        caller
            .call(authority, "hub.events.publish", json!({}))
            .await
    });
    let (call, _) = next(&mut socket).await;
    assert_eq!(call["t"], "call");
    assert_eq!(call["seq"], 3);
    socket
        .send(frame(
            &json!({"t":"reply","seq":2,"id":call["id"]}),
            br#"{"result":{"cursor":1,"inserted":true}}"#,
        ))
        .await
        .unwrap();
    assert!(task.await.unwrap().is_ok());
    for (seq, body) in [(3, "before"), (4, "after")] {
        socket
            .send(frame(
                &json!({"t":"msg","seq":seq,"s":session,"pt":"text"}),
                body.as_bytes(),
            ))
            .await
            .unwrap();
    }
    assert_eq!(daemon.await.unwrap(), ["before", "after"]);
    let (closed, _) = next(&mut socket).await;
    assert_eq!(closed["t"], "close");
    assert_eq!(closed["seq"], 4);
    hub.stop().await;
}

#[tokio::test]
async fn call_waits_for_reply_after_ack_and_resumes_without_resending_an_admitted_job() {
    let (_relay, hub, listener, _local) = connector().await;
    let mut first = ready(&hub, &listener).await;
    let caller = hub.clone();
    let authority = scope(&hub);
    let task = tokio::spawn(async move {
        caller
            .call(authority, "hub.events.publish", json!({"eventId":"stable"}))
            .await
    });
    let (call, payload) = next(&mut first).await;
    assert_eq!(call["t"], "call");
    assert_eq!(call["seq"], 1);
    assert_eq!(call["method"], "hub.events.publish");
    assert_eq!(
        serde_json::from_slice::<Value>(&payload).unwrap()["eventId"],
        "stable"
    );
    first
        .send(frame(&json!({"t":"ack","a":1}), b""))
        .await
        .unwrap();
    first
        .send(Message::Ping(Bytes::from_static(b"processed")))
        .await
        .unwrap();
    loop {
        if matches!(first.next().await, Some(Ok(Message::Pong(_)))) {
            break;
        }
    }
    assert!(!task.is_finished(), "transport ACK is not a business reply");
    let link = hub.status().await.link.unwrap();
    drop(first);
    let (mut resumed, credential) = accept(&listener).await;
    assert_eq!(credential, format!("Bearer {RESUME}"));
    next(&mut resumed).await;
    let mut header = welcome(link, true, 1);
    header["center_methods"] = json!(["hub.events.publish"]);
    resumed.send(frame(&header, b"")).await.unwrap();
    resumed
        .send(frame(
            &json!({"t":"reply","seq":1,"id":call["id"]}),
            br#"{"result":{"cursor":7,"inserted":true}}"#,
        ))
        .await
        .unwrap();
    assert_eq!(
        timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap()["cursor"],
        7
    );
    hub.stop().await;
}

#[tokio::test]
async fn fresh_logical_link_fails_pending_calls_for_idempotent_application_retry() {
    let (_relay, hub, listener, _local) = connector().await;
    let mut first = ready(&hub, &listener).await;
    let caller = hub.clone();
    let authority = scope(&hub);
    let task = tokio::spawn(async move {
        caller
            .call(authority, "hub.events.publish", json!({}))
            .await
    });
    next(&mut first).await;
    drop(first);
    let (mut fresh, _) = accept(&listener).await;
    next(&mut fresh).await;
    fresh
        .send(frame(&welcome(Uuid::new_v4(), false, 0), b""))
        .await
        .unwrap();
    assert_eq!(
        timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap(),
        Err(CallError::Unavailable)
    );
    hub.stop().await;
}

#[tokio::test]
async fn legacy_centers_wrong_authority_oversized_calls_and_capacity_are_bounded() {
    let (_relay, hub, listener, _local) = connector().await;
    let mut socket = online(&hub, &listener, 120).await;
    assert_eq!(
        hub.call(scope(&hub), "hub.events.publish", json!({})).await,
        Err(CallError::Unsupported)
    );
    hub.stop().await;
    drop(socket);

    let (_relay, hub, listener, _local) = connector().await;
    socket = ready(&hub, &listener).await;
    let mut wrong = scope(&hub);
    wrong.node_session_id = Uuid::new_v4();
    assert_eq!(
        hub.call(wrong, "hub.events.publish", json!({})).await,
        Err(CallError::Unavailable)
    );
    assert_eq!(
        hub.call(
            scope(&hub),
            "hub.events.publish",
            json!("x".repeat(wire::MAX_PAYLOAD))
        )
        .await,
        Err(CallError::Protocol)
    );
    let mut tasks = Vec::new();
    for _ in 0..16 {
        let caller = hub.clone();
        let authority = scope(&hub);
        tasks.push(tokio::spawn(async move {
            caller
                .call(authority, "hub.events.publish", json!({}))
                .await
        }));
        next(&mut socket).await;
    }
    assert_eq!(
        hub.call(scope(&hub), "hub.events.publish", json!({})).await,
        Err(CallError::Capacity)
    );
    hub.stop().await;
    for task in tasks {
        assert_eq!(
            timeout(Duration::from_secs(3), task)
                .await
                .unwrap()
                .unwrap(),
            Err(CallError::Unavailable)
        );
    }
}
