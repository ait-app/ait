use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;

use super::*;
use crate::protocol::DownloadAck;

fn tls_fixture() -> (Dialer, Arc<rustls::ServerConfig>) {
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_owned()]).unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let server = rustls::ServerConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(signing_key.serialize_der()).into(),
        )
        .unwrap();
    let mut roots = RootCertStore::empty();
    roots.add(cert.der().clone()).unwrap();
    let mut dialer = Dialer::default();
    Arc::get_mut(&mut dialer.tls)
        .unwrap()
        .dangerous()
        .set_certificate_verifier(
            rustls::client::WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider)
                .build()
                .unwrap(),
        );
    (dialer, Arc::new(server))
}

#[tokio::test]
async fn repeated_wss_connections_resume_tls_and_disable_nagle() {
    let (dialer, config) = tls_fixture();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("wss://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let acceptor = tokio_rustls::TlsAcceptor::from(config);
        let mut handshakes = Vec::new();
        for _ in 0..2 {
            let (tcp, _) = listener.accept().await.unwrap();
            let tls = acceptor.accept(tcp).await.unwrap();
            handshakes.push(tls.get_ref().1.handshake_kind().unwrap());
            let mut socket = accept_async(tls).await.unwrap();
            socket.send(Message::Text("ready".into())).await.unwrap();
            assert_eq!(
                socket.next().await.unwrap().unwrap(),
                Message::Text("ack".into())
            );
        }
        handshakes
    });
    for ticket in ["control-ticket", "data-ticket"] {
        let mut socket = dialer.connect(&url, ticket, 1024).await.unwrap();
        assert_eq!(text(&mut socket).await.unwrap(), "ready");
        let MaybeTlsStream::Rustls(tls) = socket.get_ref() else {
            panic!("WSS must use rustls");
        };
        assert!(tls.get_ref().0.nodelay().unwrap());
        send(&mut socket, Message::Text("ack".into()))
            .await
            .unwrap();
    }
    assert_eq!(
        server.await.unwrap(),
        [rustls::HandshakeKind::Full, rustls::HandshakeKind::Resumed]
    );
}

#[tokio::test]
async fn default_trust_rejects_a_self_signed_relay_certificate() {
    let (_, config) = tls_fixture();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("wss://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        assert!(
            tokio_rustls::TlsAcceptor::from(config)
                .accept(tcp)
                .await
                .is_err()
        );
    });
    assert!(matches!(
        Dialer::default().connect(&url, "ticket", 1024).await,
        Err(Error::Transport)
    ));
    server.await.unwrap();
}

#[tokio::test]
async fn typed_receive_answers_pings_and_send_preserves_frames() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        socket.send(Message::Ping(vec![1, 2].into())).await.unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap(),
            Message::Pong(vec![1, 2].into())
        );
        socket.send(Message::Pong(Vec::new().into())).await.unwrap();
        socket
            .send(Message::Text(r#"{"type":"download.complete"}"#.into()))
            .await
            .unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap(),
            Message::Binary(vec![3, 4].into())
        );
        socket.send(Message::Binary(vec![5].into())).await.unwrap();
    });
    let mut socket = Dialer::default()
        .connect(&format!("ws://{address}/data"), "ticket", 1024)
        .await
        .unwrap();
    let MaybeTlsStream::Plain(tcp) = socket.get_ref() else {
        panic!("loopback socket must be plain TCP");
    };
    assert!(tcp.nodelay().unwrap());
    let _: DownloadAck = timeout(Duration::from_secs(3), receive(&mut socket))
        .await
        .unwrap()
        .unwrap();
    send(&mut socket, Message::Binary(vec![3, 4].into()))
        .await
        .unwrap();
    assert!(matches!(text(&mut socket).await, Err(Error::Transport)));
    server.await.unwrap();
}

#[tokio::test]
async fn upgrade_rejects_invalid_credentials_and_bounded_messages() {
    assert!(matches!(
        Dialer::default()
            .connect("invalid url", "ticket", 1024)
            .await,
        Err(Error::InvalidGrant)
    ));
    assert!(matches!(
        Dialer::default()
            .connect("ws://127.0.0.1:1", "bad\nticket", 1024)
            .await,
        Err(Error::InvalidGrant)
    ));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(tcp).await.unwrap();
        socket
            .send(Message::Text("x".repeat(65).into()))
            .await
            .unwrap();
    });
    let mut socket = Dialer::default()
        .connect(&format!("ws://{address}"), "ticket", 64)
        .await
        .unwrap();
    assert!(matches!(text(&mut socket).await, Err(Error::Transport)));
    server.await.unwrap();
}
