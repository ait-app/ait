use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::http::{Response, StatusCode};

use super::*;

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
    for ticket in ["main-ticket", "bulk-ticket"] {
        let mut socket = dialer.connect(&url, ticket, 1024).await.unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap(),
            Message::Text("ready".into())
        );
        let MaybeTlsStream::Rustls(tls) = socket.get_ref() else {
            panic!("WSS must use rustls");
        };
        assert!(tls.get_ref().0.nodelay().unwrap());
        socket.send(Message::Text("ack".into())).await.unwrap();
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
    let MaybeTlsStream::Plain(tcp) = socket.get_ref() else {
        panic!("loopback socket must be plain TCP");
    };
    assert!(tcp.nodelay().unwrap());
    assert!(matches!(socket.next().await, Some(Err(_))));
    server.await.unwrap();
}

#[tokio::test]
#[allow(clippy::result_large_err)]
async fn refused_credentials_are_invalid_grants_and_other_failures_transport_errors() {
    for (status, refused) in [
        (StatusCode::UNAUTHORIZED, true),
        (StatusCode::FORBIDDEN, true),
        (StatusCode::SERVICE_UNAVAILABLE, false),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let _ = tokio_tungstenite::accept_hdr_async(tcp, |_: &_, _| {
                Err(Response::builder().status(status).body(None).unwrap())
            })
            .await;
        });
        let result = Dialer::default()
            .connect(&format!("ws://{address}"), "ticket", 1024)
            .await;
        if refused {
            assert!(matches!(result, Err(Error::InvalidGrant)), "{status}");
        } else {
            assert!(matches!(result, Err(Error::Transport)), "{status}");
        }
        server.await.unwrap();
    }
}

#[tokio::test]
async fn stalled_upgrade_obeys_the_connection_deadline() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        Dialer::default()
            .connect(&format!("ws://{address}"), "fixture", 1024)
            .await
    });
    let (_tcp, _) = listener.accept().await.unwrap();
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(6)).await;
    assert!(matches!(task.await.unwrap(), Err(Error::Transport)));
}
