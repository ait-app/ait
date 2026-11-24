use model::peer::Identity;

use super::*;
use crate::hub::bridge::PeerRefusal;
use crate::test_support::{Accepted, FakeApi, PeerCall};

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
    assert_eq!(reply["error"]["code"], -32_001);
    assert_eq!(reply["error"]["code"], crate::hub::peer::NOT_ACCEPTING);
    assert!(reply["error"]["message"].is_string());
    assert_eq!(
        next(&mut socket).await.0,
        json!({"t":"close","seq":8,"s":refused,"code":"peer_disabled"})
    );

    fill_both_quotas(&mut socket, &mut local, &from).await;
    // Only the one request ever reached the API.
    assert!(calls.try_recv().is_err());
    hub.stop().await;
}

/// Starting at center sequence 8 and Host sequence 9: eight peer sessions fill
/// their own quota, sixteen owner sessions still fit beside them, and a peer
/// session fits beside those once one of the eight ends.
async fn fill_both_quotas(socket: &mut Center, local: &mut Connections, from: &Identity) {
    // Eight peer sessions fill their own quota.
    let mut peers = Vec::new();
    for seq in 8..16 {
        let s = Uuid::new_v4();
        socket
            .send(peer_open(seq, s, from, None))
            .await
            .expect("peer open");
        assert_eq!(next(socket).await.0["t"], "accept");
        peers.push(s);
    }
    let ninth = Uuid::new_v4();
    socket
        .send(peer_open(16, ninth, from, None))
        .await
        .expect("peer open");
    assert_eq!(
        next(socket).await.0,
        json!({"t":"close","seq":17,"s":ninth,"code":"capacity"})
    );

    // All sixteen owner sessions still fit beside them, and only those count against the sixteen.
    let mut owners = Vec::new();
    for seq in 17..33 {
        owners.push(accept_owner(socket, local, seq).await);
    }
    let seventeenth = Uuid::new_v4();
    socket
        .send(open(33, seventeenth, HELLO.as_bytes()))
        .await
        .expect("owner open");
    assert_eq!(
        next(socket).await.0,
        json!({"t":"close","seq":34,"s":seventeenth,"code":"capacity"})
    );

    // With sixteen owner sessions open, a peer session fits once one of the eight ends.
    socket
        .send(frame(
            &json!({"t":"close","seq":34,"s":peers[0],"code":"done"}),
            b"",
        ))
        .await
        .expect("close");
    let later = Uuid::new_v4();
    socket
        .send(peer_open(35, later, from, None))
        .await
        .expect("peer open");
    assert_eq!(
        next(socket).await,
        (json!({"t":"accept","seq":35,"s":later}), VERSION.to_vec())
    );
}

/// Open an owner RPC session with center sequence `seq` and answer its hello as the API.
async fn accept_owner(center: &mut Center, local: &mut Connections, seq: u64) -> Accepted {
    let s = Uuid::new_v4();
    center
        .send(open(seq, s, HELLO.as_bytes()))
        .await
        .expect("owner open");
    let mut api = timeout(Duration::from_secs(3), local.recv())
        .await
        .expect("owner session in time")
        .expect("owner session connects");
    api.next().await.expect("hello");
    api.reply("server info").await;
    let (header, info) = next(center).await;
    assert_eq!((&header["t"], &header["s"]), (&json!("accept"), &json!(s)));
    assert_eq!(info, b"server info");
    api
}

#[tokio::test]
async fn a_center_close_ends_a_peer_session_without_cancelling_its_calls() {
    let (_relay, hub, center, _local, mut calls) = peer_connector().await;
    let mut socket = online(&hub, &center, 120).await;
    let from = identity();
    let s = Uuid::new_v4();
    socket
        .send(peer_open(1, s, &from, None))
        .await
        .expect("peer open");
    assert_eq!(next(&mut socket).await.0["t"], "accept");
    let request = json!({"id":1,"tool":"agent_wait","arguments":{"agentId":"b-7"}});
    socket.send(text(2, s, &request)).await.expect("request");
    let call = next_call(&mut calls).await;

    socket
        .send(frame(
            &json!({"t":"close","seq":3,"s":s,"code":"peer_gone"}),
            b"",
        ))
        .await
        .expect("close");
    settle(&mut socket).await;
    // The call still runs; its answer has nowhere to go.
    call.reply
        .send(Ok(json!({"content":[],"isError":false})))
        .expect("the call was not cancelled");
    // The route is gone: a late request reaches nobody.
    socket
        .send(text(
            4,
            s,
            &json!({"id":2,"tool":"agent_list","arguments":{}}),
        ))
        .await
        .expect("late request");

    // No reply was sent before the next session's accept.
    let later = Uuid::new_v4();
    socket
        .send(peer_open(5, later, &from, None))
        .await
        .expect("peer open");
    assert_eq!(
        next(&mut socket).await,
        (json!({"t":"accept","seq":2,"s":later}), VERSION.to_vec())
    );
    assert!(calls.try_recv().is_err());
    hub.stop().await;
}

#[tokio::test]
async fn a_peer_session_runs_a_bounded_number_of_calls_and_credits_no_more() {
    let (_relay, hub, center, _local, mut calls) = peer_connector().await;
    let mut socket = online(&hub, &center, 120).await;
    let from = identity();
    let s = Uuid::new_v4();
    socket
        .send(peer_open(1, s, &from, Some(65_536)))
        .await
        .expect("peer open");
    assert_eq!(next(&mut socket).await.0["t"], "accept");

    let requests: Vec<Value> = (1..=MAX_PEER_CALLS + 2)
        .map(|id| json!({"id":id,"tool":"agent_wait","arguments":{"agentId":format!("b-{id}")}}))
        .collect();
    let lengths: Vec<u64> = requests
        .iter()
        .map(|request| request.to_string().len() as u64)
        .collect();
    for (seq, request) in (2..).zip(&requests) {
        socket.send(text(seq, s, request)).await.expect("request");
    }
    let mut running = Vec::with_capacity(MAX_PEER_CALLS);
    for _ in 0..MAX_PEER_CALLS {
        running.push(next_call(&mut calls).await);
    }
    // Credit covers exactly the calls that run; the rest wait unread.
    let bounded = lengths[..MAX_PEER_CALLS].iter().sum::<u64>();
    let mut credit = 0;
    while credit != bounded {
        let (header, _) = next(&mut socket).await;
        assert_eq!(header["t"], "credit");
        credit = header["c"].as_u64().expect("credit");
        assert!(
            credit <= bounded,
            "credit {credit} beyond the running calls"
        );
    }
    settle(&mut socket).await;
    assert!(calls.try_recv().is_err(), "no call runs beyond the cap");

    // Answering one call frees one slot: its reply leaves, then one more call is read and credited.
    running
        .remove(0)
        .reply
        .send(Ok(json!({"content":[]})))
        .expect("reply");
    assert_eq!(next(&mut socket).await.0["t"], "msg");
    assert_eq!(
        next(&mut socket).await.0,
        json!({"t":"credit","s":s,"c":bounded + lengths[MAX_PEER_CALLS]})
    );
    let admitted = next_call(&mut calls).await;
    assert_eq!(
        admitted.request.arguments,
        json!({"agentId":format!("b-{}", MAX_PEER_CALLS + 1)})
    );
    settle(&mut socket).await;
    assert!(calls.try_recv().is_err(), "still no more than the cap");
    hub.stop().await;
}
