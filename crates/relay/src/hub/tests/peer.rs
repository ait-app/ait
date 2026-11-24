use model::peer::{Error as PeerError, Host, HostState, Identity, Peers, Request};

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

/// `hello.f` of a main link whose center declared `peer`, which the welcome enables.
const PEER_MAIN: [&str; 4] = ["lease", "credit", "download", "peer"];

/// A welcome enabling `peer` and advertising `methods`.
fn peer_welcome(link: Uuid, resumed: bool, recv: u64, methods: &[&str]) -> Value {
    let mut header = welcome(link, resumed, recv);
    header["f"] = json!(PEER_MAIN);
    header["center_methods"] = json!(methods);
    header
}

/// Accept the first connection of `hub` and welcome link `link` with `peer` enabled.
async fn peer_online(
    hub: &Connector,
    center: &TcpListener,
    link: Uuid,
    methods: &[&str],
) -> Center {
    let (mut socket, _) = accept(center).await;
    assert_eq!(next(&mut socket).await.0["f"], json!(PEER_MAIN));
    socket
        .send(frame(&peer_welcome(link, false, 0, methods), b""))
        .await
        .expect("welcome");
    next_status(hub, |status| status.online).await;
    socket
}

type Outcome = tokio::task::JoinHandle<Result<Value, PeerError>>;

/// The time limit of every call; no socket test reaches it.
const LIMIT: Duration = Duration::from_secs(60);

/// Call `tool` on the Host on `node` from a task, within [`LIMIT`].
fn call(
    hub: &Connector,
    node: Uuid,
    tool: &str,
    arguments: Value,
    caller: Option<&str>,
) -> Outcome {
    let hub = hub.clone();
    let request = Request {
        tool: tool.to_owned(),
        arguments,
        caller: caller.map(str::to_owned),
    };
    tokio::spawn(async move { Peers::call(&hub, node, request, LIMIT).await })
}

async fn outcome(call: Outcome) -> Result<Value, PeerError> {
    timeout(Duration::from_secs(3), call)
        .await
        .expect("call ends in time")
        .expect("call task")
}

/// The next frame, which must be the `open` of a peer session to `node`; returns its session.
async fn opened(center: &mut Center, seq: u64, node: Uuid) -> Uuid {
    let (header, hello) = next(center).await;
    let s: Uuid = serde_json::from_value(header["s"].clone()).expect("session id");
    assert_eq!(
        header,
        json!({"t":"open","seq":seq,"s":s,"to":{"node":node},"k":"peer","w":credit::WINDOW})
    );
    assert_eq!(hello, VERSION);
    s
}

fn peer_accept(seq: u64, s: Uuid, window: Option<u32>) -> Message {
    let mut header = json!({"t":"accept","seq":seq,"s":s});
    if let Some(window) = window {
        header["w"] = json!(window);
    }
    frame(&header, VERSION)
}

fn close(seq: u64, s: Uuid, code: &str) -> Message {
    frame(&json!({"t":"close","seq":seq,"s":s,"code":code}), b"")
}

/// The next frame, which must be a request on session `s`; returns its JSON.
async fn request(center: &mut Center, s: Uuid) -> Value {
    let (header, body) = next(center).await;
    assert_eq!((&header["t"], &header["s"]), (&json!("msg"), &json!(s)));
    assert_eq!(header["pt"], "text");
    serde_json::from_slice(&body).expect("JSON request")
}

/// Assert the Host sends nothing but acknowledgements before answering a ping.
async fn quiet(center: &mut Center) {
    center
        .send(Message::Ping(Bytes::from_static(b"quiet")))
        .await
        .expect("ping");
    timeout(Duration::from_secs(3), async {
        loop {
            match center.next().await.expect("open").expect("healthy") {
                Message::Pong(_) => return,
                Message::Binary(bytes) => {
                    let length = usize::from(u16::from_be_bytes([bytes[0], bytes[1]]));
                    let header: Value =
                        serde_json::from_slice(&bytes[2..2 + length]).expect("header");
                    assert_eq!(header["t"], "ack", "unexpected frame {header}");
                }
                _ => {}
            }
        }
    })
    .await
    .expect("pong");
}

#[tokio::test]
async fn a_call_opens_one_session_per_host_and_matches_replies_by_id() {
    let (_relay, hub, center, _local, _calls) = peer_connector().await;
    let mut socket = peer_online(&hub, &center, Uuid::new_v4(), &[]).await;
    let node = Uuid::new_v4();
    let get = call(
        &hub,
        node,
        "agent_get",
        json!({"agentId":"b-7"}),
        Some("a-1"),
    );
    let list = call(&hub, node, "agent_list", json!({}), None);

    let s = opened(&mut socket, 1, node).await;
    socket
        .send(peer_accept(1, s, Some(65_536)))
        .await
        .expect("accept");
    // Both calls share the session: the next two frames are their requests, not a second open.
    let mut requests = [request(&mut socket, s).await, request(&mut socket, s).await];
    requests.sort_by_key(|request| request["tool"].as_str().map(str::to_owned));
    let [get_request, list_request] = requests;
    let get_id = get_request["id"].as_u64().expect("id");
    let list_id = list_request["id"].as_u64().expect("id");
    assert_eq!(
        get_request,
        json!({"id":get_id,"tool":"agent_get","arguments":{"agentId":"b-7"},"caller":{"agentId":"a-1"}})
    );
    assert_eq!(
        list_request,
        json!({"id":list_id,"tool":"agent_list","arguments":{}})
    );
    assert_eq!(
        [get_id.min(list_id), get_id.max(list_id)],
        [1, 2],
        "ids count up within the session"
    );

    // Replies in the opposite order still reach their own calls.
    let replies = [
        json!({"id":list_id,"result":{"structuredContent":{"agents":[]}}}),
        json!({"id":get_id,"result":{"structuredContent":{"agent":"b-7"}}}),
    ];
    for (seq, reply) in (2..).zip(&replies) {
        socket.send(text(seq, s, reply)).await.expect("reply");
    }
    assert_eq!(
        outcome(get).await,
        Ok(json!({"structuredContent":{"agent":"b-7"}}))
    );
    assert_eq!(
        outcome(list).await,
        Ok(json!({"structuredContent":{"agents":[]}}))
    );
    // Each delivered reply is credited back.
    let delivered = replies
        .iter()
        .map(|reply| reply.to_string().len() as u64)
        .sum::<u64>();
    let mut credit = 0;
    while credit != delivered {
        let (header, _) = next(&mut socket).await;
        assert_eq!((&header["t"], &header["s"]), (&json!("credit"), &json!(s)));
        credit = header["c"].as_u64().expect("credit");
    }

    // A call to another Host opens its own session.
    let other = Uuid::new_v4();
    let _pending = call(&hub, other, "agent_list", json!({}), None);
    assert_ne!(opened(&mut socket, 4, other).await, s);
    hub.stop().await;
}

#[tokio::test]
async fn refusals_before_the_request_map_to_their_errors() {
    let (_relay, hub, center, _local, _calls) = peer_connector().await;
    let mut socket = peer_online(&hub, &center, Uuid::new_v4(), &[]).await;
    let refusals = [
        ("unknown_target", PeerError::Offline),
        ("unsupported", PeerError::Unsupported),
        ("peer_disabled", PeerError::NotAccepting),
        ("capacity", PeerError::Capacity),
        ("forbidden", PeerError::Unavailable),
        ("invalid_window", PeerError::Unavailable),
    ];
    for ((seq, (code, error)), node) in (1..)
        .zip(refusals)
        .zip(std::iter::repeat_with(Uuid::new_v4))
    {
        let pending = call(&hub, node, "agent_list", json!({}), None);
        let s = opened(&mut socket, seq, node).await;
        socket.send(close(seq, s, code)).await.expect("close");
        assert_eq!(outcome(pending).await, Err(error), "{code}");
    }
    // No request was ever sent.
    quiet(&mut socket).await;
    hub.stop().await;
}

#[tokio::test]
async fn a_ninth_host_is_refused_without_an_open() {
    let (_relay, hub, center, _local, _calls) = peer_connector().await;
    let mut socket = peer_online(&hub, &center, Uuid::new_v4(), &[]).await;
    let mut pending = Vec::new();
    for seq in 1..=8 {
        let node = Uuid::new_v4();
        pending.push(call(&hub, node, "agent_list", json!({}), None));
        opened(&mut socket, seq, node).await;
    }
    let ninth = call(&hub, Uuid::new_v4(), "agent_list", json!({}), None);
    assert_eq!(outcome(ninth).await, Err(PeerError::Capacity));
    quiet(&mut socket).await;
    hub.stop().await;
}

#[tokio::test]
async fn error_replies_map_to_their_errors() {
    let (_relay, hub, center, _local, _calls) = peer_connector().await;
    let mut socket = peer_online(&hub, &center, Uuid::new_v4(), &[]).await;
    let node = Uuid::new_v4();
    let errors = [
        (-32_001, PeerError::NotAccepting),
        (-32_000, PeerError::TooLarge),
        (
            -32_602,
            PeerError::Rejected {
                code: -32_602,
                message: "workspaceId is required".to_owned(),
            },
        ),
    ];
    let first = call(&hub, node, "agent_create", json!({}), None);
    let s = opened(&mut socket, 1, node).await;
    socket.send(peer_accept(1, s, None)).await.expect("accept");
    let mut pending = Some(first);
    for (seq, (code, error)) in (2..).zip(errors) {
        let current = pending
            .take()
            .unwrap_or_else(|| call(&hub, node, "agent_create", json!({}), None));
        let id = request(&mut socket, s).await["id"].clone();
        let reply = json!({"id":id,"error":{"code":code,"message":"workspaceId is required"}});
        socket.send(text(seq, s, &reply)).await.expect("reply");
        assert_eq!(outcome(current).await, Err(error), "{code}");
    }
    hub.stop().await;
}

#[tokio::test]
async fn an_abandoned_open_is_closed() {
    let (_relay, hub, center, _local, _calls) = peer_connector().await;
    let mut socket = peer_online(&hub, &center, Uuid::new_v4(), &[]).await;
    let node = Uuid::new_v4();
    let abandoned = call(&hub, node, "agent_list", json!({}), None);
    let s = opened(&mut socket, 1, node).await;
    // The caller gives up while the target is still in its grace period.
    abandoned.abort();
    assert_eq!(
        next(&mut socket).await.0,
        json!({"t":"close","seq":2,"s":s,"code":"done"})
    );
    // A late accept of the closed session is ignored; a new call opens a new session.
    socket.send(peer_accept(1, s, None)).await.expect("accept");
    let _pending = call(&hub, node, "agent_list", json!({}), None);
    assert_ne!(opened(&mut socket, 3, node).await, s);
    hub.stop().await;
}

#[tokio::test]
async fn a_session_that_ends_after_the_request_reports_an_unknown_outcome() {
    let (_relay, hub, center, _local, _calls) = peer_connector().await;
    let mut socket = peer_online(&hub, &center, Uuid::new_v4(), &[]).await;
    let node = Uuid::new_v4();
    let create = call(
        &hub,
        node,
        "agent_create",
        json!({"idempotencyKey":"k"}),
        None,
    );
    let s = opened(&mut socket, 1, node).await;
    socket.send(peer_accept(1, s, None)).await.expect("accept");
    assert_eq!(request(&mut socket, s).await["tool"], "agent_create");

    socket.send(close(2, s, "peer_gone")).await.expect("close");
    assert_eq!(outcome(create).await, Err(PeerError::OutcomeUnknown));

    // The next call opens a new session.
    let _pending = call(&hub, node, "agent_list", json!({}), None);
    assert_ne!(opened(&mut socket, 3, node).await, s);
    hub.stop().await;
}

#[tokio::test]
async fn calls_survive_a_resumed_link() {
    let (_relay, hub, center, _local, _calls) = peer_connector().await;
    let link = Uuid::new_v4();
    let mut first = peer_online(&hub, &center, link, &[]).await;
    let node = Uuid::new_v4();
    let wait = call(&hub, node, "agent_wait", json!({"agentId":"b-7"}), None);
    let get = call(&hub, node, "agent_get", json!({"agentId":"b-8"}), None);
    let s = opened(&mut first, 1, node).await;
    first
        .send(peer_accept(1, s, Some(65_536)))
        .await
        .expect("accept");
    let requests = [next(&mut first).await, next(&mut first).await];
    assert_eq!(
        requests.each_ref().map(|(header, _)| header["seq"].clone()),
        [json!(2), json!(3)]
    );

    // The connection drops after the center stored only the open.
    drop(first);
    let (mut second, credential) = accept(&center).await;
    assert_eq!(credential, format!("Bearer {RESUME}"));
    assert_eq!(
        next(&mut second).await.0,
        json!({"t":"hello","v":1,"resume":{"link":link,"recv":1},"f":PEER_MAIN})
    );
    second
        .send(frame(&peer_welcome(link, true, 1, &[]), b""))
        .await
        .expect("resume");
    // Both requests are replayed unchanged, in their original order.
    assert_eq!([next(&mut second).await, next(&mut second).await], requests);

    let ids = requests.each_ref().map(|(_, body)| {
        let request: Value = serde_json::from_slice(body).expect("request");
        (request["tool"].clone(), request["id"].clone())
    });
    for (seq, (tool, id)) in (2..).zip(ids) {
        let result = json!({"content":[tool]});
        second
            .send(text(seq, s, &json!({"id":id,"result":result})))
            .await
            .expect("reply");
    }
    assert_eq!(outcome(wait).await, Ok(json!({"content":["agent_wait"]})));
    assert_eq!(outcome(get).await, Ok(json!({"content":["agent_get"]})));
    hub.stop().await;
}

#[tokio::test]
async fn a_held_request_is_sent_once_credit_admits_it() {
    let (_relay, hub, center, _local, _calls) = peer_connector().await;
    let mut socket = peer_online(&hub, &center, Uuid::new_v4(), &[]).await;
    let node = Uuid::new_v4();
    let large = json!({"text":"x".repeat(40_000)});
    let _first = call(&hub, node, "agent_prompt", large.clone(), None);
    let s = opened(&mut socket, 1, node).await;
    socket
        .send(peer_accept(1, s, Some(65_536)))
        .await
        .expect("accept");
    let sent = request(&mut socket, s).await;
    // The second request does not fit beside the first in the target's window.
    let _second = call(&hub, node, "agent_prompt", large, None);
    quiet(&mut socket).await;

    let credit = sent.to_string().len() as u64;
    socket
        .send(frame(&json!({"t":"credit","s":s,"c":credit}), b""))
        .await
        .expect("credit");
    assert_eq!(request(&mut socket, s).await["id"], 2);
    hub.stop().await;
}

/// Next frame from either Host as (header, payload); acknowledgements and
/// hop-local messages are skipped.
async fn read(center: &mut Center) -> (Value, Vec<u8>) {
    loop {
        let Message::Binary(bytes) = center.next().await.expect("open").expect("healthy") else {
            continue;
        };
        let length = usize::from(u16::from_be_bytes([bytes[0], bytes[1]]));
        let header: Value = serde_json::from_slice(&bytes[2..2 + length]).expect("header");
        if header["t"] != "ack" {
            return (header, bytes[2 + length..].to_vec());
        }
    }
}

/// `header` as the center forwards it to the other Host: sequenced again in
/// that Host's own sequence space, and an `open` addressed from `from`.
fn forwarded(mut header: Value, seq: &mut u64, from: Uuid, identity: &Identity) -> Value {
    if header["t"] == "credit" {
        return header;
    }
    *seq += 1;
    header["seq"] = json!(*seq);
    if header["t"] == "open" {
        let fields = header.as_object_mut().expect("header object");
        fields.remove("to");
        header["from"] = json!(from);
        header["peer"] =
            json!({"host_id":identity.host_id,"server_id":identity.server_id,"name":identity.name});
    }
    header
}

#[tokio::test]
async fn two_links_complete_a_call_through_a_forwarding_center() {
    let (_relay_a, a, center_a, _local_a, _calls_a) = peer_connector().await;
    let (_relay_b, b, center_b, _local_b, mut calls_b) = peer_connector().await;
    let mut socket_a = peer_online(&a, &center_a, Uuid::new_v4(), &[]).await;
    let mut socket_b = peer_online(&b, &center_b, Uuid::new_v4(), &[]).await;
    let (node_a, node_b) = (Uuid::new_v4(), Uuid::new_v4());
    let identity_a = identity();

    let pending = call(
        &a,
        node_b,
        "agent_get",
        json!({"agentId":"b-7"}),
        Some("a-1"),
    );
    let expected = identity_a.clone();
    let api_b = tokio::spawn(async move {
        let call = next_call(&mut calls_b).await;
        assert_eq!(call.from, expected);
        assert_eq!(
            call.request,
            Request {
                tool: "agent_get".to_owned(),
                arguments: json!({"agentId":"b-7"}),
                caller: Some("a-1".to_owned()),
            }
        );
        call.reply
            .send(Ok(
                json!({"structuredContent":{"agent":"b-7","status":"idle"}}),
            ))
            .expect("reply");
    });

    let (mut to_a, mut to_b) = (0, 0);
    let mut pending = std::pin::pin!(pending);
    let result = loop {
        tokio::select! {
            outcome = &mut pending => break outcome.expect("call task"),
            (header, payload) = read(&mut socket_a) => {
                if header["t"] == "open" {
                    assert_eq!(header["to"], json!({"node":node_b}));
                }
                let header = forwarded(header, &mut to_b, node_a, &identity_a);
                socket_b.send(frame(&header, &payload)).await.expect("to B");
            }
            (header, payload) = read(&mut socket_b) => {
                let header = forwarded(header, &mut to_a, node_b, &identity_a);
                socket_a.send(frame(&header, &payload)).await.expect("to A");
            }
        }
    };
    assert_eq!(
        result,
        Ok(json!({"structuredContent":{"agent":"b-7","status":"idle"}}))
    );
    api_b.await.expect("B answered");
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn hosts_come_from_the_center_and_are_unavailable_without_the_method() {
    let (_relay, hub, center, _local, _calls) = peer_connector().await;
    let mut socket = peer_online(&hub, &center, Uuid::new_v4(), &["hub.peers.list"]).await;
    let ids: Vec<[Uuid; 3]> = (0..3)
        .map(|_| [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()])
        .collect();
    let listed = json!({"hosts":[
        {"host_id":ids[0][0],"node_id":ids[0][1],"server_id":ids[0][2],"name":"Studio",
            "hostname":"studio.local","platform":"macos","app_version":"1.2.3","state":"online",
            "peer":{"supported":true,"enabled":true}},
        {"host_id":ids[1][0],"node_id":ids[1][1],"server_id":ids[1][2],"name":"Build",
            "hostname":"build","platform":"linux","app_version":"1.2.2","state":"reconnecting",
            "peer":{"supported":true,"enabled":false}},
        {"host_id":ids[2][0],"node_id":ids[2][1],"server_id":ids[2][2],"name":"Laptop",
            "hostname":null,"platform":null,"app_version":null,"state":"offline",
            "peer":{"supported":false,"enabled":false}},
    ]});
    let connector = hub.clone();
    let hosts = tokio::spawn(async move { Peers::hosts(&connector).await });
    let (header, params) = next(&mut socket).await;
    assert_eq!(
        (&header["t"], &header["method"]),
        (&json!("call"), &json!("hub.peers.list"))
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&params).expect("params"),
        json!({})
    );
    socket
        .send(frame(
            &json!({"t":"reply","seq":1,"id":header["id"]}),
            json!({"result":listed}).to_string().as_bytes(),
        ))
        .await
        .expect("reply");
    let hosts = timeout(Duration::from_secs(3), hosts)
        .await
        .expect("hosts in time")
        .expect("hosts task")
        .expect("hosts");
    let host = |index: usize,
                name: &str,
                hostname: &str,
                platform: &str,
                version: &str,
                state,
                supported,
                enabled| Host {
        host_id: ids[index][0],
        node_id: ids[index][1],
        server_id: ids[index][2],
        name: name.to_owned(),
        hostname: hostname.to_owned(),
        platform: platform.to_owned(),
        app_version: version.to_owned(),
        state,
        supported,
        enabled,
    };
    assert_eq!(
        hosts,
        [
            host(
                0,
                "Studio",
                "studio.local",
                "macos",
                "1.2.3",
                HostState::Online,
                true,
                true
            ),
            host(
                1,
                "Build",
                "build",
                "linux",
                "1.2.2",
                HostState::Reconnecting,
                true,
                false
            ),
            host(2, "Laptop", "", "", "", HostState::Offline, false, false),
        ]
    );
    // Within five seconds on the same link the list is answered without a call.
    let cached = timeout(Duration::from_secs(3), Peers::hosts(&hub))
        .await
        .expect("cached hosts in time");
    assert_eq!(cached, Ok(hosts));
    hub.stop().await;

    let (_relay, hub, center, _local, _calls) = peer_connector().await;
    let mut socket = peer_online(&hub, &center, Uuid::new_v4(), &[]).await;
    assert_eq!(Peers::hosts(&hub).await, Err(PeerError::Unavailable));
    quiet(&mut socket).await;
    hub.stop().await;
}
