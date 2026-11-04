use super::*;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::{Api, ConfigError, Services};

const TOKEN: &str = "test-server-listener-token-32-characters";

#[test]
fn authorities_are_limited_to_the_destination_and_loopback_alias() {
    let local = LocalAddress(
        "[::ffff:127.0.0.1]:80".parse().unwrap(),
        "127.0.0.1:1".parse().unwrap(),
    );
    assert_eq!(
        local.authorities(),
        ["127.0.0.1:80", "127.0.0.1", "localhost:80", "localhost"]
    );
    assert_eq!(
        allowed_authorities("192.168.1.2:7316".parse().unwrap()),
        ["192.168.1.2:7316"]
    );
    assert_eq!(
        allowed_authorities("[::1]:80".parse().unwrap()),
        ["[::1]:80", "[::1]", "localhost:80", "localhost"]
    );
}

#[test]
fn an_unknown_destination_admits_no_host() {
    let local = LocalAddress(
        SocketAddr::from(([0, 0, 0, 0], 0)),
        "127.0.0.1:1".parse().unwrap(),
    );
    assert!(local.authorities().is_empty());
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("host", "0.0.0.0:0".parse().unwrap());
    let rejected = crate::auth::validate_source(
        &headers,
        &local.authorities(),
        &crate::browser_auth::BrowserAuth::default(),
    )
    .unwrap_err();
    assert_eq!(rejected.0, axum::http::StatusCode::FORBIDDEN);
}

#[test]
fn accepts_network_addresses_but_requires_an_assigned_port() {
    for address in ["0.0.0.0:7316", "[::]:7316", "192.168.1.2:7316"] {
        assert!(api(address.parse().unwrap()).is_ok());
    }
    assert!(matches!(
        api("127.0.0.1:0".parse().unwrap()),
        Err(ConfigError::InvalidAddress)
    ));
}

#[tokio::test]
async fn wildcard_listeners_validate_destination_origin_and_credentials() {
    for (bind, connect) in [("0.0.0.0:0", "127.0.0.1"), ("[::]:0", "[::1]")] {
        let listener = TcpListener::bind(bind).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let destination: SocketAddr = format!("{connect}:{port}").parse().unwrap();
        let api = api(listener.local_addr().unwrap()).unwrap();
        let router = api
            .router()
            .into_make_service_with_connect_info::<LocalAddress>();
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

        assert_eq!(
            request(destination, &destination.to_string(), TOKEN, None).await,
            200
        );
        assert_eq!(
            request(destination, &format!("localhost:{port}"), TOKEN, None).await,
            200
        );
        assert_eq!(
            request(destination, &destination.to_string(), "wrong", None).await,
            401
        );
        for host in [
            format!("evil.example:{port}"),
            format!("192.0.2.1:{port}"),
            format!("{connect}:1"),
        ] {
            assert_eq!(request(destination, &host, TOKEN, None).await, 403);
        }
        for (origin, expected) in [
            (format!("http://{destination}"), 200),
            ("http://evil.example".to_owned(), 403),
        ] {
            assert_eq!(
                request(destination, &destination.to_string(), TOKEN, Some(&origin)).await,
                expected
            );
        }
        task.abort();
        let _ = task.await;
    }
}

fn api(address: SocketAddr) -> Result<Api, ConfigError> {
    Api::new(
        address,
        "test-server".to_owned(),
        "test-instance".to_owned(),
        TOKEN.into(),
        Services::default(),
    )
}

async fn request(address: SocketAddr, host: &str, token: &str, origin: Option<&str>) -> u16 {
    let mut socket = TcpStream::connect(address).await.unwrap();
    let origin = origin
        .map(|value| format!("Origin: {value}\r\n"))
        .unwrap_or_default();
    socket.write_all(format!("GET /v1/server/info HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {token}\r\n{origin}Connection: close\r\n\r\n").as_bytes()).await.unwrap();
    let mut response = String::new();
    socket.read_to_string(&mut response).await.unwrap();
    response.split_whitespace().nth(1).unwrap().parse().unwrap()
}
