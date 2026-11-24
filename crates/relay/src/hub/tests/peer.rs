use model::peer::Identity;

use super::*;
use crate::hub::bridge::PeerRefusal;
use crate::test_support::{FakeApi, PeerCall};

type PeerCalls = mpsc::UnboundedReceiver<PeerCall>;

const VERSION: &[u8] = br#"{"v":1}"#;

/// A connector whose center declares `peer`, serving with an API that hands
/// calls from other Hosts to the test.
async fn peer_connector() -> (
    crate::Connector,
    Connector,
    TcpListener,
    Connections,
    PeerCalls,
) {
    let mut features = declared(&DECLARED);
    features.push("peer".to_owned());
    let (api, connections, calls) = FakeApi::with_peers();
    let local = "127.0.0.1:9".parse().expect("local address");
    let (relay, hub, center) = granted(local, features, None, api).await;
    (relay, hub, center, connections, calls)
}

fn identity() -> Identity {
    Identity {
        host_id: Uuid::new_v4(),
        server_id: Uuid::new_v4(),
        name: "Studio".to_owned(),
    }
}

/// The center's `open` of a peer session from `from`.
fn peer_open(seq: u64, s: Uuid, from: &Identity, window: Option<u32>) -> Message {
    let mut header = json!({"t":"open","seq":seq,"s":s,"from":Uuid::new_v4(),"k":"peer",
        "peer":{"host_id":from.host_id,"server_id":from.server_id,"name":from.name}});
    if let Some(window) = window {
        header["w"] = json!(window);
    }
    frame(&header, VERSION)
}

fn text(seq: u64, s: Uuid, body: &Value) -> Message {
    frame(
        &json!({"t":"msg","seq":seq,"s":s,"pt":"text"}),
        body.to_string().as_bytes(),
    )
}

async fn next_call(calls: &mut PeerCalls) -> PeerCall {
    timeout(Duration::from_secs(3), calls.recv())
        .await
        .expect("peer call in time")
        .expect("api alive")
}

/// Next `msg` from the Host with its JSON payload; records the credit frames before it.
async fn next_message(center: &mut Center, credits: &mut Vec<u64>) -> (Value, Value) {
    loop {
        let (header, payload) = next(center).await;
        if header["t"] == "credit" {
            credits.push(header["c"].as_u64().expect("credit"));
            continue;
        }
        let payload = serde_json::from_slice(&payload).expect("JSON payload");
        return (header, payload);
    }
}

#[tokio::test]
async fn peer_requests_reach_the_api_with_the_centers_identity() {
    let (_relay, hub, center, _local, mut calls) = peer_connector().await;
    let mut socket = online(&hub, &center, 120).await;
    let from = identity();
    let s = Uuid::new_v4();
    socket
        .send(peer_open(1, s, &from, Some(65_536)))
        .await
        .expect("peer open");
    assert_eq!(
        next(&mut socket).await,
        (
            json!({"t":"accept","seq":1,"s":s,"w":credit::WINDOW}),
            VERSION.to_vec()
        )
    );

    let first = json!({"id":1,"tool":"agent_list","arguments":{},"caller":{"agentId":"a-1"}});
    let second = json!({"id":2,"tool":"agent_get","arguments":{"agentId":"b-7"}});
    let delivered = [&first, &second]
        .iter()
        .map(|request| request.to_string().len() as u64)
        .sum::<u64>();
    socket.send(text(2, s, &first)).await.expect("request 1");
    socket.send(text(3, s, &second)).await.expect("request 2");

    // Each request runs in its own task, so they may reach the API in either order.
    let mut received = [next_call(&mut calls).await, next_call(&mut calls).await];
    received.sort_by(|left, right| left.request.tool.cmp(&right.request.tool));
    let [get, list] = received;
    assert_eq!((&get.from, &list.from), (&from, &from));
    assert_eq!(
        (
            &list.request.tool,
            &list.request.arguments,
            &list.request.caller
        ),
        (
            &"agent_list".to_owned(),
            &json!({}),
            &Some("a-1".to_owned())
        )
    );
    assert_eq!(
        (
            &get.request.tool,
            &get.request.arguments,
            &get.request.caller
        ),
        (&"agent_get".to_owned(), &json!({"agentId":"b-7"}), &None)
    );

    // Replies go out as they complete, matched to their requests by id.
    let mut credits = Vec::new();
    get.reply
        .send(Ok(json!({"structuredContent":{"agent":"b-7"}})))
        .expect("reply 2");
    let (header, reply) = next_message(&mut socket, &mut credits).await;
    assert_eq!(header, json!({"t":"msg","seq":2,"s":s,"pt":"text"}));
    assert_eq!(
        reply,
        json!({"id":2,"result":{"structuredContent":{"agent":"b-7"}}})
    );
    list.reply
        .send(Ok(json!({"content":[],"isError":false})))
        .expect("reply 1");
    let (header, reply) = next_message(&mut socket, &mut credits).await;
    assert_eq!(header, json!({"t":"msg","seq":3,"s":s,"pt":"text"}));
    assert_eq!(
        reply,
        json!({"id":1,"result":{"content":[],"isError":false}})
    );

    // Both requests are credited back once delivered.
    while credits.last() != Some(&delivered) {
        let (header, _) = next(&mut socket).await;
        assert_eq!(header["t"], "credit");
        credits.push(header["c"].as_u64().expect("credit"));
    }
    hub.stop().await;
}

#[tokio::test]
async fn malformed_peer_opens_and_refused_calls_close_the_session() {
    let (_relay, hub, center, mut local, mut calls) = peer_connector().await;
    let mut socket = online(&hub, &center, 120).await;
    let from = identity();
    let peer = json!({"host_id":from.host_id,"server_id":from.server_id,"name":from.name});
    let visitor = json!({"user_id":Uuid::new_v4(),"display_name":"Bob","group_id":Uuid::new_v4(),
        "share_id":Uuid::new_v4(),"resource_kind":"node","resource_key":"","permission":"operate"});
    let malformed = [
        (json!({"k":"peer"}), VERSION),
        (json!({"k":"peer","peer":peer}), br#"{"v":2}"#.as_slice()),
        (json!({"k":"peer","peer":peer}), b"hello".as_slice()),
        (json!({"k":"peer","peer":peer,"visitor":visitor}), VERSION),
        (
            json!({"k":"peer","peer":peer,"service":{"kind":"automation"}}),
            VERSION,
        ),
    ];
    for (seq, (fields, payload)) in (1..).zip(malformed) {
        let s = Uuid::new_v4();
        let mut header = json!({"t":"open","seq":seq,"s":s,"from":Uuid::new_v4()});
        for (name, value) in fields.as_object().expect("fields") {
            header[name] = value.clone();
        }
        socket.send(frame(&header, payload)).await.expect("open");
        assert_eq!(
            next(&mut socket).await.0,
            json!({"t":"close","seq":seq,"s":s,"code":"unsupported"}),
            "{fields}"
        );
    }

    // A refusal for a Host that stopped accepting is answered, then ends the session.
    let refused = Uuid::new_v4();
    socket
        .send(peer_open(6, refused, &from, None))
        .await
        .expect("peer open");
    assert_eq!(next(&mut socket).await.0["t"], "accept");
    socket
        .send(text(
            7,
            refused,
            &json!({"id":9,"tool":"agent_list","arguments":{}}),
        ))
        .await
        .expect("request");
    let call = next_call(&mut calls).await;
    assert_eq!(call.request.tool, "agent_list");
    call.reply
        .send(Err(PeerRefusal::Disabled))
        .expect("refusal");
    let (header, reply) = next(&mut socket).await;
    assert_eq!(header, json!({"t":"msg","seq":7,"s":refused,"pt":"text"}));
    let reply: Value = serde_json::from_slice(&reply).expect("JSON reply");
    assert_eq!(reply["id"], 9);
    assert_eq!(reply["error"]["code"], -32_000);
    assert!(reply["error"]["message"].is_string());
    assert_eq!(
        next(&mut socket).await.0,
        json!({"t":"close","seq":8,"s":refused,"code":"peer_disabled"})
    );

    // Eight peer sessions fill their own quota; the owner's sessions are unaffected.
    for seq in 8..16 {
        socket
            .send(peer_open(seq, Uuid::new_v4(), &from, None))
            .await
            .expect("peer open");
        assert_eq!(next(&mut socket).await.0["t"], "accept");
    }
    let ninth = Uuid::new_v4();
    socket
        .send(peer_open(16, ninth, &from, None))
        .await
        .expect("peer open");
    assert_eq!(
        next(&mut socket).await.0,
        json!({"t":"close","seq":17,"s":ninth,"code":"capacity"})
    );
    let owner = Uuid::new_v4();
    socket
        .send(open(17, owner, HELLO.as_bytes()))
        .await
        .expect("owner open");
    let mut api = local.recv().await.expect("owner session connects");
    api.next().await.expect("hello");
    api.reply("server info").await;
    assert_eq!(
        next(&mut socket).await,
        (
            json!({"t":"accept","seq":18,"s":owner}),
            b"server info".to_vec()
        )
    );
    // Only the one request ever reached the API.
    assert!(calls.try_recv().is_err());
    hub.stop().await;
}
