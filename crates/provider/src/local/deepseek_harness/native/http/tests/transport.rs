use std::net::SocketAddr;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::*;
use crate::ports::diagnostics::Category;

fn api_at(address: SocketAddr) -> Api {
    Api {
        base: Url::parse(&format!("http://{address}/")).unwrap(),
        cookie: header::HeaderValue::from_static("fixture=private"),
        client: Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap(),
        diagnostics: crate::local::diagnostics::Diagnostics::new("deepseek-harness"),
    }
}

#[tokio::test]
async fn refused_loopback_connection_records_the_connect_stage() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api = api_at(listener.local_addr().unwrap());
    drop(listener);

    assert_eq!(
        api.call("permissionPresets/catalog", json!({})).await,
        Err(AgentSessionError::Failed)
    );
    let diagnostic = api.diagnostics.failure().unwrap();
    assert_eq!(diagnostic.category, Category::Network);
    assert_eq!(diagnostic.source, "http_connect");
}

#[tokio::test]
async fn dropped_request_and_truncated_response_record_distinct_stages_without_replaying() {
    for (response, stage) in [
        (None, "http_send"),
        (
            Some("HTTP/1.1 200 OK\r\nContent-Length: 1024\r\nConnection: close\r\n\r\n{"),
            "http_body",
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api = api_at(listener.local_addr().unwrap());
        let (finished, completed) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            read_request(&mut socket).await;
            if let Some(response) = response {
                socket.write_all(response.as_bytes()).await.unwrap();
                socket.shutdown().await.unwrap();
            }
            drop(socket);
            tokio::select! {
                _ = completed => {},
                _ = listener.accept() => panic!("an uncertain native request must not be replayed"),
            }
        });

        assert_eq!(
            api.call("session/create", json!({})).await,
            Err(AgentSessionError::Failed)
        );
        let diagnostic = api.diagnostics.failure().unwrap();
        assert_eq!(diagnostic.category, Category::Network);
        assert_eq!(diagnostic.source, stage);
        finished.send(()).unwrap();
        server.await.unwrap();
    }
}

#[tokio::test]
async fn stalled_response_body_preserves_timeout_classification() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut api = api_at(listener.local_addr().unwrap());
    api.client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(1))
        .build()
        .unwrap();
    let (finished, completed) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1024\r\n\r\n{")
            .await
            .unwrap();
        completed.await.unwrap();
    });

    assert_eq!(
        api.call("permissionPresets/catalog", json!({})).await,
        Err(AgentSessionError::Failed)
    );
    let diagnostic = api.diagnostics.failure().unwrap();
    assert_eq!(diagnostic.category, Category::Timeout);
    assert_eq!(diagnostic.source, "http_body");
    finished.send(()).unwrap();
    server.await.unwrap();
}

async fn read_request(socket: &mut tokio::net::TcpStream) {
    let mut request = Vec::new();
    let mut chunk = [0; 1024];
    loop {
        let count = socket.read(&mut chunk).await.unwrap();
        assert_ne!(
            count, 0,
            "request must finish before the fixture disconnects"
        );
        request.extend_from_slice(&chunk[..count]);
        if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            let headers = std::str::from_utf8(&request[..end]).unwrap();
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            if request.len() >= end + 4 + length {
                break;
            }
        }
    }
}
