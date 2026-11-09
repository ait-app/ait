use std::time::Duration;

use serde_json::{Value, json};
use tokio::time::timeout;

use model::share::Visitor;

use super::*;
use crate::test_support::{Workspaces, address};

fn hello(capabilities: &Value, single: bool) -> Value {
    json!({"type":"hello", "client_id":"hub-session",
        "protocol":{"major":1,"min_minor":0,"max_minor":4},
        "capabilities":capabilities,
        "required_capabilities": if single { json!(["connection.single.v1"]) } else { json!([]) }})
}

async fn send(connection: &Connection, value: &Value) {
    connection
        .sender
        .send(Frame::Text(value.to_string().into()))
        .await
        .expect("API reads the connection");
}

async fn receive(connection: &mut Connection) -> Value {
    let frame = timeout(Duration::from_secs(3), connection.receiver.recv())
        .await
        .expect("reply in time")
        .expect("connection open");
    let Frame::Text(text) = frame else {
        std::panic!("text reply expected");
    };
    serde_json::from_slice(&text).expect("json reply")
}

async fn request(connection: &mut Connection, method: &str, params: Value) -> Value {
    send(
        connection,
        &json!({"type":"request","request_id":"r1","method":method,"params":params}),
    )
    .await;
    loop {
        let reply = receive(connection).await;
        if reply["request_id"] == "r1" {
            return reply;
        }
    }
}

async fn closed(receiver: &mut mpsc::Receiver<Frame>) -> bool {
    timeout(Duration::from_secs(3), async {
        while receiver.recv().await.is_some() {}
    })
    .await
    .is_ok()
}

#[tokio::test]
async fn owner_sessions_speak_the_business_protocol_in_both_connection_modes() {
    let fixture = Workspaces::new(address());
    let bridge = InProcess::new(&fixture.api.shared);
    for single in [false, true] {
        let mut connection = bridge.connect(Admission::Owner).await.expect("admitted");
        send(&connection, &hello(&json!(["connection.ping"]), single)).await;
        let info = receive(&mut connection).await;
        assert_eq!(info["type"], "server_info");
        assert!(
            info["info"]["features"]
                .as_array()
                .expect("features")
                .contains(&json!(model::server::single::FEATURE))
        );
        let ping = request(&mut connection, "connection.ping", json!({"nonce":"n"})).await;
        assert_eq!(ping["result"]["nonce"], "n");

        let Connection {
            sender,
            mut receiver,
        } = connection;
        drop(sender);
        assert!(closed(&mut receiver).await, "closing the session ends it");
    }
}

#[tokio::test]
async fn guest_sessions_are_confined_and_end_when_their_share_is_revoked() {
    let fixture = Workspaces::new(address());
    let bridge = InProcess::new(&fixture.api.shared);
    let visitor = fixture.accept_share().await;
    let stranger = Visitor {
        resource_key: fixture.other.clone(),
        ..visitor.clone()
    };
    assert!(matches!(
        bridge.connect(Admission::Guest(&stranger)).await,
        Err(Refused)
    ));

    let mut guest = bridge
        .connect(Admission::Guest(&visitor))
        .await
        .expect("admitted");
    send(
        &guest,
        &hello(&json!(["connection.ping", "workspace.get.request"]), false),
    )
    .await;
    assert_eq!(receive(&mut guest).await["type"], "server_info");
    let other = request(
        &mut guest,
        "workspace.get.request",
        json!({"workspaceId": fixture.other}),
    )
    .await;
    assert_eq!(other["code"], "forbidden");
    let ping = request(&mut guest, "connection.ping", json!({"nonce":"g"})).await;
    assert_eq!(ping["result"]["nonce"], "g");

    let mut owner = bridge.connect(Admission::Owner).await.expect("admitted");
    send(&owner, &hello(&json!(["share.remove.request"]), false)).await;
    assert_eq!(receive(&mut owner).await["type"], "server_info");
    let removed = request(
        &mut owner,
        "share.remove.request",
        json!({"resourceKind":"workspace","resourceKey":fixture.shared,"groupId":fixture.group}),
    )
    .await;
    assert_eq!(removed["result"]["removed"], true);

    assert!(
        closed(&mut guest.receiver).await,
        "revoked guest session stayed open"
    );
    assert!(matches!(
        bridge.connect(Admission::Guest(&visitor)).await,
        Err(Refused)
    ));
}

#[tokio::test]
async fn draining_or_dropped_apis_refuse_sessions() {
    let fixture = Workspaces::new(address());
    let bridge = InProcess::new(&fixture.api.shared);
    fixture.api.begin_shutdown();
    assert!(matches!(
        bridge.connect(Admission::Owner).await,
        Err(Refused)
    ));
    drop(fixture);
    assert!(matches!(
        bridge.connect(Admission::Owner).await,
        Err(Refused)
    ));
}

#[test]
fn control_frames_have_no_in_process_equivalent() {
    assert_eq!(
        frame(Message::Text("t".into())),
        Some(Frame::Text("t".into()))
    );
    assert_eq!(frame(Message::Ping(Vec::new().into())), None);
    assert_eq!(frame(Message::Close(None)), None);
    assert!(message(Frame::Text(vec![0xff].into())).is_err());
    assert!(matches!(
        message(Frame::Binary(vec![1].into())),
        Ok(Message::Binary(bytes)) if bytes.as_ref() == [1]
    ));
}

#[tokio::test(start_paused = true)]
async fn a_session_waiting_for_its_peer_is_not_closed_by_the_socket_write_deadline() {
    let fixture = Workspaces::new(address());
    let bridge = InProcess::new(&fixture.api.shared);
    let mut connection = bridge.connect(Admission::Owner).await.expect("admitted");
    send(&connection, &hello(&json!(["connection.ping"]), false)).await;
    assert_eq!(receive(&mut connection).await["type"], "server_info");
    let pings = QUEUE + 8;
    for nonce in 0..pings {
        send(
            &connection,
            &json!({"type":"request","request_id":format!("p{nonce}"),
                "method":"connection.ping","params":{"nonce":nonce.to_string()}}),
        )
        .await;
    }

    // Nothing reads the session for longer than a socket may stall.
    tokio::time::sleep(WRITE_STALL).await;

    for nonce in 0..pings {
        let reply = receive(&mut connection).await;
        assert_eq!(reply["result"]["nonce"], nonce.to_string());
    }
}

/// Longer than the WebSocket write deadline.
const WRITE_STALL: Duration = Duration::from_secs(30);

/// An API with a Bot service over one Workspace, and that Workspace's Bot set up.
async fn with_bot() -> (tempfile::TempDir, crate::Api, ::bot::service::Bots) {
    use crate::test_support::bots::{WORKSPACE, service};
    let root = tempfile::tempdir().expect("root");
    let bots = service(root.path());
    let api = crate::Api::new(
        address(),
        "test-server".into(),
        "instance".into(),
        crate::test_support::TOKEN.into(),
        crate::Services {
            bot: Some(bots.clone()),
            ..crate::Services::default()
        },
    )
    .expect("api");
    bots.execute(
        "bot.setup.request",
        json!({"workspaceId":WORKSPACE,"execution":{"provider":"test"}}),
    )
    .await
    .expect("bot");
    (root, api, bots)
}

/// Create the Workspace Bot's Hub channel, enabled or not.
async fn hub_channel(bots: &::bot::service::Bots, enabled: bool) -> Value {
    let id = domain::ChannelId::generate();
    bots.execute(
        "bot.channel.create.request",
        json!({"workspaceId":crate::test_support::bots::WORKSPACE,"channelId":id,
            "config":{"kind":"hub"},"enabled":enabled}),
    )
    .await
    .expect("hub channel");
    json!(id)
}

/// The methods the center offers in its automation hello.
fn center_hello() -> Value {
    hello(
        &json!([
            "connection.single.v1",
            "bot.hub.message.send.request",
            "bot.turn.get.request",
            "bot.turn.cancel.request",
            "bot.hub.delivery.ack.request",
            "bot.hub.conversation.close.request",
            "bot.message.send.request",
            "share.list.request"
        ]),
        true,
    )
}

/// An admitted automation session after its hello, with the enabled Hub channel's ID.
async fn center(api: &crate::Api, bots: &::bot::service::Bots) -> (Connection, Value, InProcess) {
    use crate::test_support::bots::WORKSPACE;
    let bridge = InProcess::new(&api.shared);
    assert!(
        matches!(bridge.connect(Admission::Automation).await, Err(Refused)),
        "no Hub channel yet"
    );
    let channel = hub_channel(bots, false).await;
    assert!(
        matches!(bridge.connect(Admission::Automation).await, Err(Refused)),
        "a Hub channel starts disabled"
    );
    bots.execute(
        "bot.channel.update.request",
        json!({"workspaceId":WORKSPACE,"channelId":channel,"enabled":true}),
    )
    .await
    .expect("enable");
    let mut connection = bridge
        .connect(Admission::Automation)
        .await
        .expect("admitted");
    send(&connection, &center_hello()).await;
    assert_eq!(receive(&mut connection).await["type"], "server_info");
    (connection, channel, bridge)
}

#[tokio::test]
async fn automation_sessions_post_into_the_hub_channel_and_poll_its_replies() {
    use crate::test_support::bots::WORKSPACE;
    let (_root, api, bots) = with_bot().await;
    let (mut connection, channel, _bridge) = center(&api, &bots).await;
    assert_eq!(
        api.hub_bots().expect("Bot service").borrow()[0].workspace_id,
        WORKSPACE
    );
    let accepted = request(
        &mut connection,
        "bot.hub.message.send.request",
        json!({"workspaceId":WORKSPACE,"channelId":channel,"peer":"telegram:tg","sender":"hub",
            "thread":"c1","messageId":"automation:1","text":"fix the build"}),
    )
    .await;
    assert_eq!(accepted["result"]["accepted"], true, "{accepted}");
    let turn = accepted["result"]["turn"]["id"].clone();
    let done = timeout(Duration::from_secs(5), async {
        loop {
            let got = request(
                &mut connection,
                "bot.turn.get.request",
                json!({"workspaceId":WORKSPACE,"turnId":turn}),
            )
            .await;
            if got["result"]["turn"]["status"] == "completed" {
                break got;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the Bot answers");
    assert_eq!(done["result"]["turn"]["reply"], "Bot answer");
    assert_eq!(done["result"]["turn"]["delivery"], "pending");
    let acked = request(
        &mut connection,
        "bot.hub.delivery.ack.request",
        json!({"workspaceId":WORKSPACE,"turnId":turn,"status":"delivered"}),
    )
    .await;
    assert_eq!(acked["result"], json!({}), "{acked}");
    let closed = request(
        &mut connection,
        "bot.hub.conversation.close.request",
        json!({"workspaceId":WORKSPACE,"conversationId":done["result"]["turn"]["conversationId"]}),
    )
    .await;
    assert_eq!(closed["result"], json!({}), "{closed}");
}

#[tokio::test]
async fn automation_sessions_reach_nothing_else_and_end_when_the_hub_channel_closes() {
    use crate::test_support::bots::WORKSPACE;
    let (_root, api, bots) = with_bot().await;
    let (mut connection, channel, _bridge) = center(&api, &bots).await;
    let opened = bots
        .execute(
            "bot.conversation.open.request",
            json!({"workspaceId":WORKSPACE}),
        )
        .await
        .expect("owner conversation");
    let owner = bots
        .execute(
            "bot.message.send.request",
            json!({"workspaceId":WORKSPACE,"conversationId":opened["conversation"]["id"],
                "messageId":"owner","text":"hi"}),
        )
        .await
        .expect("owner turn");
    for (method, params) in [
        (
            "bot.turn.get.request",
            json!({"workspaceId":WORKSPACE,"turnId":owner["turn"]["id"]}),
        ),
        (
            "bot.message.send.request",
            json!({"workspaceId":WORKSPACE,"conversationId":opened["conversation"]["id"],
                "messageId":"m","text":"hi"}),
        ),
        ("share.list.request", json!({})),
    ] {
        let refused = request(&mut connection, method, params).await;
        assert_eq!(refused["code"], "forbidden", "{method}: {refused}");
    }
    send(
        &connection,
        &json!({"type":"event","method":"terminal.input","params":{}}),
    )
    .await;
    assert_eq!(receive(&mut connection).await["code"], "forbidden");

    bots.execute(
        "bot.channel.update.request",
        json!({"workspaceId":WORKSPACE,"channelId":channel,"enabled":false}),
    )
    .await
    .expect("disable");
    let Connection {
        sender: _sender,
        mut receiver,
    } = connection;
    assert!(
        closed(&mut receiver).await,
        "disabling the last Hub channel ends its sessions"
    );
    assert!(api.hub_bots().expect("Bot service").borrow().is_empty());
}

#[tokio::test]
async fn webhook_channels_ask_the_hosts_serve_link_for_their_endpoint() {
    use crate::test_support::bots::WORKSPACE;
    let (_root, _api, bots) = with_bot().await;
    // The API attached its serve link; without a center session it is offline.
    let created = bots
        .execute(
            "bot.channel.webhook.create.request",
            json!({"workspaceId":WORKSPACE}),
        )
        .await
        .expect("create");
    assert_eq!(created, json!({"state":"failed","reason":"centerOffline"}));
}
