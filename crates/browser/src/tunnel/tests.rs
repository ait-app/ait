use super::*;
use model::outbound::{Frame, Outbound};
use model::{Lifecycle, Request, Runtime, ServerInfo};
use tokio::net::TcpListener;

fn runtime() -> Runtime {
    Runtime::new(ServerInfo {
        server_id: "host".into(),
        version: None,
        instance_id: "instance".into(),
        listen: "127.0.0.1:0".into(),
        lifecycle: Lifecycle::Ready,
        protocol: model::server::VERSION,
        capabilities: vec![],
        implemented_capabilities: vec![],
        features: vec![],
        limits: model::server::Limits::default(),
    })
}

async fn request(tunnels: &mut Tunnels, runtime: &Runtime, method: &str, params: Value) -> Value {
    let (outbound, mut receiver) = Outbound::new();
    tunnels
        .dispatch(Context {
            request: Request {
                id: "request".into(),
                method: method.into(),
                params,
            },
            runtime,
            outbound: &outbound,
            available_subscriptions: 16 - tunnels.len(),
        })
        .unwrap();
    let Frame::Text(text) = timeout(Duration::from_secs(2), receiver.recv())
        .await
        .unwrap()
        .unwrap()
        .message
    else {
        panic!("JSON response expected")
    };
    serde_json::from_str(&text).unwrap()
}

#[test]
fn targets_allow_names_and_all_ip_ranges_for_daemon_side_connections() {
    for (host, expected) in [
        ("localhost", "127.0.0.1"),
        ("APP.LOCALHOST.", "127.0.0.1"),
        ("127.2.3.4", "127.2.3.4"),
        ("[::1]", "::1"),
        ("192.168.1.1", "192.168.1.1"),
        ("169.254.1.1", "169.254.1.1"),
        ("203.0.113.1", "203.0.113.1"),
        ("Example.COM", "example.com"),
        ("localhost.example.com", "localhost.example.com"),
        ("[2001:db8::1]", "2001:db8::1"),
        ("fe80::1", "fe80::1"),
        ("::ffff:127.0.0.1", "::ffff:127.0.0.1"),
    ] {
        assert_eq!(tunnel_host(host).unwrap(), expected);
    }
}

#[test]
fn targets_reject_empty_names_and_url_components() {
    for host in [
        "",
        ".",
        "https://example.com",
        "example.com:80",
        "user@host",
        "a b",
        "[::1",
    ] {
        assert_eq!(tunnel_host(host), Err(ErrorCode::InvalidMessage));
    }
    assert_eq!(
        tunnel_host(&"a".repeat(254)),
        Err(ErrorCode::InvalidMessage)
    );
}

#[tokio::test]
async fn remote_loopback_http_and_half_close_use_the_connection_owned_tunnel() {
    for host in ["127.0.0.1", "app.localhost", "::ffff:127.0.0.1"] {
        http_roundtrip(host).await;
    }
}

async fn http_roundtrip(host: &str) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        socket.read_to_end(&mut request).await.unwrap();
        assert_eq!(request, b"GET / HTTP/1.0\r\n\r\n");
        socket
            .write_all(b"HTTP/1.0 200 OK\r\n\r\nremote workspace")
            .await
            .unwrap();
    });
    let runtime = runtime();
    let mut tunnels = Tunnels::default();
    let opened = request(
        &mut tunnels,
        &runtime,
        "browser.tunnel.open.request",
        json!({"host":host, "port":port}),
    )
    .await;
    let id = opened["result"]["subscriptionId"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(tunnels.len(), 1);
    let sent = request(&mut tunnels, &runtime, "browser.tunnel.write.request", json!({"subscriptionId":id,"dataBase64": STANDARD.encode(b"GET / HTTP/1.0\r\n\r\n"),"eof":true})).await;
    assert!(sent.get("result").is_some());
    let received = request(
        &mut tunnels,
        &runtime,
        "browser.tunnel.read.request",
        json!({"subscriptionId":id}),
    )
    .await;
    assert_eq!(
        STANDARD
            .decode(received["result"]["dataBase64"].as_str().unwrap())
            .unwrap(),
        b"HTTP/1.0 200 OK\r\n\r\nremote workspace"
    );
    let eof = request(
        &mut tunnels,
        &runtime,
        "browser.tunnel.read.request",
        json!({"subscriptionId":id}),
    )
    .await;
    assert_eq!(eof["result"]["eof"], true);
    tunnels.release(&id);
    assert_eq!(tunnels.len(), 0);
    assert_eq!(
        request(
            &mut tunnels,
            &runtime,
            "browser.tunnel.read.request",
            json!({"subscriptionId":id})
        )
        .await["code"],
        "subscription_not_found"
    );
    server.await.unwrap();
}

#[tokio::test]
async fn validation_capacity_and_ownership_are_enforced() {
    let runtime = runtime();
    let mut tunnels = Tunnels::default();
    for params in [
        json!({"host":"https://example.com","port":80}),
        json!({"host":"127.0.0.1","port":0}),
        json!({"host":"127.0.0.1","port":65536}),
    ] {
        assert_eq!(
            request(
                &mut tunnels,
                &runtime,
                "browser.tunnel.open.request",
                params
            )
            .await["code"],
            "invalid_message"
        );
    }
    assert_eq!(
        request(
            &mut tunnels,
            &runtime,
            "browser.tunnel.write.request",
            json!({"subscriptionId":"foreign","dataBase64":"AA=="})
        )
        .await["code"],
        "subscription_not_found"
    );
    assert_eq!(
        request(
            &mut tunnels,
            &runtime,
            "browser.tunnel.write.request",
            json!({"subscriptionId":"foreign","dataBase64":"?"})
        )
        .await["code"],
        "invalid_message"
    );
    assert_eq!(request(&mut tunnels, &runtime, "browser.tunnel.write.request", json!({"subscriptionId":"foreign","dataBase64":STANDARD.encode(vec![0; MAX_CHUNK_BYTES+1])})).await["code"], "invalid_message");
    let (outbound, mut receiver) = Outbound::new();
    tunnels
        .dispatch(Context {
            request: Request {
                id: "full".into(),
                method: "browser.tunnel.open.request".into(),
                params: json!({"host":"127.0.0.1","port":80}),
            },
            runtime: &runtime,
            outbound: &outbound,
            available_subscriptions: 0,
        })
        .unwrap();
    let Frame::Text(text) = receiver.recv().await.unwrap().message else {
        panic!("JSON expected")
    };
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["code"],
        "resource_exhausted"
    );
}

#[tokio::test]
async fn releasing_a_tunnel_cancels_pending_reads_and_closes_the_remote_socket() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let runtime = runtime();
    let mut tunnels = Tunnels::default();
    let opened = request(
        &mut tunnels,
        &runtime,
        "browser.tunnel.open.request",
        json!({"host":"127.0.0.1","port":port}),
    )
    .await;
    let id = opened["result"]["subscriptionId"].as_str().unwrap();
    let (mut socket, _) = listener.accept().await.unwrap();
    let (outbound, mut receiver) = Outbound::new();
    tunnels
        .dispatch(Context {
            request: Request {
                id: "read".into(),
                method: "browser.tunnel.read.request".into(),
                params: json!({"subscriptionId":id}),
            },
            runtime: &runtime,
            outbound: &outbound,
            available_subscriptions: 15,
        })
        .unwrap();
    tunnels.release(id);
    let _ = timeout(Duration::from_secs(1), receiver.recv())
        .await
        .unwrap();
    let mut data = [0];
    assert_eq!(
        timeout(Duration::from_secs(1), socket.read(&mut data))
            .await
            .unwrap()
            .unwrap(),
        0
    );
}
