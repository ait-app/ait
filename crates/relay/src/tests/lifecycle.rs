use super::*;
use tokio::net::TcpListener;
use tokio_tungstenite::{WebSocketStream, accept_async};

struct ControlFixture {
    connector: Connector,
    listener: TcpListener,
    client: Socket,
    server: WebSocketStream<tokio::net::TcpStream>,
    center: Url,
    epoch: Uuid,
}

impl ControlFixture {
    async fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let dialer = Dialer::default();
        let url = format!("ws://{address}");
        let connect = dialer.connect(&url, "fixture", 16 * 1024);
        let accept = async {
            let (tcp, _) = listener.accept().await.unwrap();
            accept_async(tcp).await.unwrap()
        };
        let (client, server) = tokio::join!(connect, accept);
        Self {
            connector: Connector::new(
                "127.0.0.1:1".parse().unwrap(),
                "local-fixture".into(),
                "server".into(),
                "instance".into(),
            ),
            listener,
            client: client.unwrap(),
            server,
            center: center_url(&format!("http://{address}")).unwrap(),
            epoch: Uuid::new_v4(),
        }
    }

    fn open(id: Uuid, epoch: Uuid) -> Message {
        Message::Text(
            json!({"type":"open_data","relay_session_id":id,
            "daemon_ticket":"a".repeat(64),"epoch":epoch,"mode":"ait-rust-single-v1"})
            .to_string()
            .into(),
        )
    }
}

#[tokio::test]
async fn control_rejects_foreign_epochs_duplicate_sessions_and_session_overload() {
    for case in ["epoch", "duplicate", "capacity"] {
        let mut fixture = ControlFixture::new().await;
        let id = Uuid::new_v4();
        let messages: Vec<_> = match case {
            "epoch" => vec![ControlFixture::open(id, Uuid::new_v4())],
            "duplicate" => vec![
                ControlFixture::open(id, fixture.epoch),
                ControlFixture::open(id, fixture.epoch),
            ],
            "capacity" => (0..17)
                .map(|_| ControlFixture::open(Uuid::new_v4(), fixture.epoch))
                .collect(),
            _ => unreachable!("fixed fixture case"),
        };
        for message in messages {
            fixture.server.send(message).await.unwrap();
        }
        let result = timeout(
            Duration::from_secs(3),
            fixture.connector.serve_commands(
                &mut fixture.client,
                &fixture.center,
                fixture.epoch,
                false,
            ),
        )
        .await
        .unwrap();
        assert!(matches!(result, Err(Error::Protocol)), "{case}");
        assert!(fixture.listener.local_addr().is_ok());
    }
}

#[tokio::test]
async fn control_answers_heartbeats_cancels_data_and_remains_available() {
    let mut fixture = ControlFixture::new().await;
    let id = Uuid::new_v4();
    fixture
        .server
        .send(ControlFixture::open(id, fixture.epoch))
        .await
        .unwrap();
    let driver = fixture.connector.serve_commands(
        &mut fixture.client,
        &fixture.center,
        fixture.epoch,
        false,
    );
    let peer = async {
        fixture
            .server
            .send(Message::Pong(vec![1].into()))
            .await
            .unwrap();
        fixture
            .server
            .send(Message::Text(
                json!({"type":"cancel_session","relay_session_id":id})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        fixture
            .server
            .send(Message::Ping(vec![2, 3].into()))
            .await
            .unwrap();
        assert_eq!(
            fixture.server.next().await.unwrap().unwrap(),
            Message::Pong(vec![2, 3].into())
        );
        fixture.server.send(Message::Close(None)).await.unwrap();
    };
    let (result, ()) = timeout(Duration::from_secs(3), async { tokio::join!(driver, peer) })
        .await
        .unwrap();
    assert!(matches!(result, Err(Error::Transport)));
}

#[tokio::test]
async fn control_read_timeout_and_binary_frames_close_the_control_connection() {
    for timed_out in [false, true] {
        let mut fixture = ControlFixture::new().await;
        if !timed_out {
            fixture
                .server
                .send(Message::Binary(vec![1].into()))
                .await
                .unwrap();
            assert!(matches!(
                fixture
                    .connector
                    .serve_commands(&mut fixture.client, &fixture.center, fixture.epoch, false)
                    .await,
                Err(Error::Transport)
            ));
            continue;
        }
        let task = tokio::spawn(async move {
            fixture
                .connector
                .serve_commands(&mut fixture.client, &fixture.center, fixture.epoch, false)
                .await
        });
        tokio::task::yield_now().await;
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(66)).await;
        assert!(matches!(task.await.unwrap(), Err(Error::Transport)));
        tokio::time::resume();
    }
}

#[test]
fn wildcard_listeners_are_replaced_by_loopback_destinations() {
    for (listen, expected) in [
        ("0.0.0.0:7316", "ws://127.0.0.1:7316/v1/ws"),
        ("[::]:7316", "ws://[::1]:7316/v1/ws"),
    ] {
        let connector = Connector::new(
            listen.parse().unwrap(),
            "fixture".into(),
            "server".into(),
            "instance".into(),
        );
        assert_eq!(connector.local.url, expected);
    }
}

#[tokio::test]
async fn failed_download_jobs_release_control_admission_and_do_not_drop_heartbeats() {
    use crate::diagnostics::tests::Capture;
    use tracing::instrument::WithSubscriber;
    let mut fixture = ControlFixture::new().await;
    let id = Uuid::new_v4();
    fixture
        .server
        .send(Message::Text(
            json!({"type":"open_data","relay_session_id":id,
        "daemon_ticket":"a".repeat(64),"epoch":fixture.epoch,"mode":"ait-download-v1",
        "download_token":"invalid token"})
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    let capture = Capture::default();
    let _subscriber = tracing::subscriber::set_default(capture.subscriber());
    let driver = fixture.connector.serve_commands(
        &mut fixture.client,
        &fixture.center,
        fixture.epoch,
        false,
    );
    let peer = async {
        loop {
            if capture.text().contains("Relay session finished") {
                break;
            }
            tokio::task::yield_now().await;
        }
        fixture
            .server
            .send(Message::Ping(vec![1].into()))
            .await
            .unwrap();
        assert_eq!(
            fixture.server.next().await.unwrap().unwrap(),
            Message::Pong(vec![1].into())
        );
        fixture.server.send(Message::Close(None)).await.unwrap();
    };
    let (result, ()) = timeout(
        Duration::from_secs(3),
        async { tokio::join!(driver, peer) }.with_subscriber(capture.subscriber()),
    )
    .await
    .unwrap();
    assert!(matches!(result, Err(Error::Transport)));
    assert!(capture.text().contains("Relay session failed"));
}

#[tokio::test]
async fn abrupt_control_disconnect_is_a_transport_failure() {
    use tokio::io::AsyncWriteExt;
    let mut fixture = ControlFixture::new().await;
    fixture.server.get_mut().shutdown().await.unwrap();
    let result = timeout(
        Duration::from_secs(3),
        fixture.connector.serve_commands(
            &mut fixture.client,
            &fixture.center,
            fixture.epoch,
            false,
        ),
    )
    .await
    .unwrap();
    assert!(matches!(result, Err(Error::Transport)));
}

#[tokio::test]
async fn tunnel_retries_do_not_extend_the_control_read_deadline() {
    let mut fixture = ControlFixture::new().await;
    let task = tokio::spawn(async move {
        fixture
            .connector
            .serve_commands(&mut fixture.client, &fixture.center, fixture.epoch, true)
            .await
    });
    // Receive the first request so the control loop and its independent idle timer are running.
    let request = fixture.server.next().await.unwrap().unwrap();
    assert!(request.to_text().unwrap().contains("tunnel.request"));
    tokio::time::pause();
    for _ in 0..13 {
        tokio::time::advance(Duration::from_secs(5)).await;
        tokio::task::yield_now().await;
    }
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(matches!(task.await.unwrap(), Err(Error::Transport)));
    tokio::time::resume();
}
