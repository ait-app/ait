//! A guest connection on a real socket is confined to its shared workspace and ends on revocation.

use model::share::{GUEST_HEADER, Visitor};
use tokio_tungstenite::tungstenite::Error as WsError;

use super::*;
use crate::test_support::{TOKEN as WORKSPACE_TOKEN, Workspaces};

struct Served {
    fixture: Workspaces,
    task: JoinHandle<()>,
}

impl Served {
    async fn start() -> (Self, Visitor) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let fixture = Workspaces::new(listener.local_addr().unwrap());
        let visitor = fixture.accept_share().await;
        let shutdown = fixture.api.clone();
        let router = fixture.api.router();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async move { shutdown.wait_draining().await })
                .await
                .unwrap();
        });
        (Self { fixture, task }, visitor)
    }

    async fn connect(&self, bearer: bool, guest: Option<&str>) -> Result<Socket, WsError> {
        let address = self.fixture.api.shared.info.listen.clone();
        let mut request = format!("ws://{address}/v1/ws")
            .into_client_request()
            .unwrap();
        if bearer {
            request.headers_mut().insert(
                "authorization",
                format!("Bearer {WORKSPACE_TOKEN}").parse().unwrap(),
            );
        }
        if let Some(guest) = guest {
            request
                .headers_mut()
                .insert(GUEST_HEADER, guest.parse().unwrap());
        }
        connect_async(request).await.map(|(socket, _)| socket)
    }

    async fn stop(self) {
        self.fixture.api.begin_shutdown();
        tokio::time::timeout(Duration::from_secs(8), async {
            self.task.await.unwrap();
            self.fixture.api.wait_closed().await;
        })
        .await
        .unwrap();
    }
}

fn status(result: Result<Socket, WsError>) -> u16 {
    match result {
        Err(WsError::Http(response)) => response.status().as_u16(),
        Err(error) => panic!("unexpected error: {error}"),
        Ok(_) => panic!("upgrade unexpectedly succeeded"),
    }
}

async fn handshake(socket: &mut Socket, capabilities: Value) {
    let mut offer = hello();
    offer["capabilities"] = capabilities;
    send(socket, offer).await;
    assert_eq!(receive(socket).await["type"], "server_info");
}

#[tokio::test]
async fn guests_reach_only_their_workspace_until_revoked() {
    let (served, visitor) = Served::start().await;
    let header = visitor.encode();
    assert_eq!(status(served.connect(false, Some(&header)).await), 403);
    assert_eq!(status(served.connect(true, Some("%%%")).await), 400);
    let stranger = Visitor {
        workspace_key: served.fixture.other.clone(),
        ..visitor.clone()
    };
    assert_eq!(
        status(served.connect(true, Some(&stranger.encode())).await),
        403
    );

    let mut owner = served.connect(true, None).await.unwrap();
    handshake(
        &mut owner,
        json!(["share.list.request", "share.remove.request"]),
    )
    .await;
    let listed = request(&mut owner, "share.list.request", json!({})).await;
    assert_eq!(listed["result"]["shares"][0]["state"], "accepted");

    let mut guest = served.connect(true, Some(&header)).await.unwrap();
    handshake(
        &mut guest,
        json!([
            "connection.ping",
            "fs.explorer.request",
            "workspace.get.request"
        ]),
    )
    .await;
    let other_path = served.fixture.root.path().join("other");
    for (method, params) in [
        ("daemon.get_status.request", json!({})),
        ("share.list.request", json!({})),
        (
            "workspace.get.request",
            json!({"workspaceId": served.fixture.other}),
        ),
        ("fs.explorer.request", json!({"cwd": other_path})),
    ] {
        let reply = request(&mut guest, method, params).await;
        assert_eq!(reply["type"], "error", "{method}");
        assert_eq!(reply["code"], "forbidden", "{method}");
        assert_eq!(reply["request_id"], "r1");
    }
    // Inside the workspace the policy admits the request; this host installs no file service.
    let inside = request(
        &mut guest,
        "fs.explorer.request",
        json!({"cwd": served.fixture.shared_path(), "mode": "list"}),
    )
    .await;
    assert_eq!(inside["code"], "not_implemented");
    let ping = request(&mut guest, "connection.ping", json!({"nonce": "n"})).await;
    assert_eq!(ping["result"]["nonce"], "n");

    let removed = request(
        &mut owner,
        "share.remove.request",
        json!({
            "workspaceKey": served.fixture.shared,
            "organizationId": served.fixture.organization,
        }),
    )
    .await;
    assert_eq!(removed["result"]["removed"], true);
    let closed = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match guest.next().await {
                None | Some(Err(_) | Ok(Message::Close(_))) => break,
                Some(Ok(_)) => {}
            }
        }
    })
    .await;
    assert!(closed.is_ok(), "revoked guest connection stayed open");
    assert_eq!(status(served.connect(true, Some(&header)).await), 403);
    served.stop().await;
}
