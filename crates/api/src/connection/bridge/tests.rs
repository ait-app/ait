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

fn enable(fixture: &Workspaces, root: &std::path::Path) {
    fixture
        .api
        .shared
        .automation
        .set(model::automation::Settings {
            enabled: true,
            roots: vec![model::automation::Root {
                path: root.to_string_lossy().into_owned(),
                label: None,
            }],
            providers: vec!["codex".to_owned()],
        })
        .expect("valid settings");
}

#[tokio::test]
async fn automation_sessions_need_enabled_settings_and_end_when_disabled() {
    let fixture = Workspaces::new(address());
    let bridge = InProcess::new(&fixture.api.shared);
    assert!(matches!(
        bridge.connect(Admission::Automation).await,
        Err(Refused)
    ));
    enable(&fixture, &fixture.shared_path());
    let mut connection = bridge
        .connect(Admission::Automation)
        .await
        .expect("admitted");
    send(
        &connection,
        &hello(
            &json!([
                "connection.ping",
                "share.list.request",
                "automation.settings.set.request"
            ]),
            true,
        ),
    )
    .await;
    assert_eq!(receive(&mut connection).await["type"], "server_info");
    for (method, params) in [
        ("automation.settings.set.request", json!({"settings":{}})),
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

    fixture
        .api
        .shared
        .automation
        .set(model::automation::Settings::default())
        .expect("disable");
    let Connection {
        sender: _sender,
        mut receiver,
    } = connection;
    assert!(
        closed(&mut receiver).await,
        "disabling automation ends its sessions"
    );
}

#[tokio::test]
async fn owners_manage_automation_settings_over_rpc() {
    let fixture = Workspaces::new(address());
    let bridge = InProcess::new(&fixture.api.shared);
    let mut owner = bridge.connect(Admission::Owner).await.expect("admitted");
    send(
        &owner,
        &hello(
            &json!([
                "automation.settings.get.request",
                "automation.settings.set.request"
            ]),
            true,
        ),
    )
    .await;
    receive(&mut owner).await;
    let empty = request(&mut owner, "automation.settings.get.request", json!({})).await;
    assert_eq!(empty["result"]["settings"]["enabled"], false);
    let root = fixture.shared_path().to_string_lossy().into_owned();
    let set = request(
        &mut owner,
        "automation.settings.set.request",
        json!({"settings":{"enabled":true,"roots":[{"path":root}],"providers":[]}}),
    )
    .await;
    assert_eq!(set["result"]["settings"]["roots"][0]["path"], json!(root));
    let invalid = request(
        &mut owner,
        "automation.settings.set.request",
        json!({"settings":{"enabled":true,"roots":[{"path":"relative"}]}}),
    )
    .await;
    assert_eq!(invalid["code"], "invalid_message");
    assert!(fixture.api.shared.automation.get().enabled);
}
