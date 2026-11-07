use serde_json::{Value, json};
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

use super::super::{SessionKind, credit, run};
use super::*;
use crate::test_support::{FILE_SIZE, local, local_file};

struct Download {
    inbound: mpsc::Sender<Frame>,
    output: mpsc::Receiver<(Uuid, Output)>,
}

fn start(
    address: std::net::SocketAddr,
    request: &Value,
    visitor: Option<model::share::Visitor>,
    credit: Option<credit::Outgoing>,
) -> Download {
    let (inbound, inbound_rx) = mpsc::channel(4);
    let (output_tx, output) = mpsc::channel(4);
    let session = Session {
        id: Uuid::new_v4(),
        local: local(address, None),
        visitor,
        inbound: inbound_rx,
        output: output_tx,
        credit,
    };
    tokio::spawn(run(
        session,
        SessionKind::Download,
        request.to_string().into(),
    ));
    Download { inbound, output }
}

impl Download {
    async fn next(&mut self) -> Output {
        timeout(Duration::from_secs(3), self.output.recv())
            .await
            .expect("output in time")
            .expect("open")
            .1
    }

    /// Read the accept and the whole stream; returns the body and the payload bytes sent.
    async fn receive(&mut self, credit: Option<&watch::Sender<u64>>) -> (Vec<u8>, u64) {
        assert_eq!(self.next().await, Output::Accept(Bytes::new()));
        let mut body = Vec::new();
        let mut sent = 0_u64;
        loop {
            let Output::Message(kind, payload) = self.next().await else {
                std::panic!("download messages only");
            };
            sent += payload.len() as u64;
            if let Some(credit) = credit {
                credit.send_replace(sent);
            }
            match kind {
                PayloadKind::Binary => {
                    assert!(payload.len() <= download::CHUNK);
                    body.extend_from_slice(&payload);
                }
                PayloadKind::Text => {
                    let message: Value = serde_json::from_slice(&payload).expect("json");
                    if message["type"] == "download.end" {
                        assert_eq!(message, json!({"type":"download.end","bytes":body.len()}));
                        return (body, sent);
                    }
                    assert_eq!(
                        message,
                        json!({"type":"download.headers","status":200,"content_length":FILE_SIZE,
                            "content_type":"application/octet-stream"})
                    );
                }
            }
        }
    }

    async fn complete(&self) {
        self.inbound
            .send(Frame::Text(r#"{"type":"download.complete"}"#.into()))
            .await
            .expect("complete");
    }
}

#[tokio::test]
async fn streams_relay_download_messages_within_the_peer_window_then_closes_done() {
    let (address, local_task) = local_file(200).await;
    let (peer, credit) = watch::channel(0);
    let mut download = start(
        address,
        &json!({"token":"scoped-token"}),
        None,
        Some(credit::Outgoing::new(credit, 65_536)),
    );

    let (body, _) = download.receive(Some(&peer)).await;
    assert_eq!(body, vec![42_u8; FILE_SIZE]);
    download.complete().await;

    assert_eq!(download.next().await, Output::Closed("done"));
    let request = local_task.await.expect("local");
    assert!(request.starts_with("GET /api/files/download?token=scoped-token HTTP/1.1\r\n"));
    assert!(
        !request
            .to_ascii_lowercase()
            .contains(model::share::DOWNLOAD_VISITOR_HEADER)
    );
}

#[tokio::test]
async fn guest_downloads_name_their_visitor_and_accept_bridged_requests() {
    let (address, local_task) = local_file(200).await;
    let visitor = model::share::Visitor {
        user_id: Uuid::new_v4(),
        display_name: None,
        username: None,
        group_id: Uuid::new_v4(),
        share_id: Uuid::new_v4(),
        resource_kind: model::share::ResourceKind::Node,
        resource_key: String::new(),
        permission: "operate".to_owned(),
    };
    let user = visitor.user_id;
    let mut download = start(
        address,
        &json!({"download_token":"scoped-token"}),
        Some(visitor),
        None,
    );

    download.receive(None).await;
    download.complete().await;

    assert_eq!(download.next().await, Output::Closed("done"));
    let request = local_task.await.expect("local").to_ascii_lowercase();
    assert!(request.contains(&format!(
        "{}: {user}",
        model::share::DOWNLOAD_VISITOR_HEADER
    )));
}

#[tokio::test]
async fn refused_requests_close_without_accepting() {
    let (address, _local) = local_file(403).await;
    let mut refused = start(address, &json!({"token":"scoped-token"}), None, None);
    assert_eq!(refused.next().await, Output::Closed("unavailable"));

    let mut malformed = start(address, &json!({"ticket":"scoped-token"}), None, None);
    assert_eq!(malformed.next().await, Output::Closed("invalid_message"));
}

#[tokio::test]
async fn a_wrong_acknowledgement_or_a_cancelled_session_ends_the_download() {
    let (address, _local) = local_file(200).await;
    let mut download = start(address, &json!({"token":"scoped-token"}), None, None);
    download.receive(None).await;
    download
        .inbound
        .send(Frame::Text(r#"{"type":"download.end"}"#.into()))
        .await
        .expect("wrong ack");
    assert_eq!(download.next().await, Output::Closed("invalid_message"));

    let (address, _local) = local_file(200).await;
    let (_peer, credit) = watch::channel(0);
    let mut cancelled = start(
        address,
        &json!({"token":"scoped-token"}),
        None,
        Some(credit::Outgoing::new(credit, 65_536)),
    );
    assert_eq!(cancelled.next().await, Output::Accept(Bytes::new()));
    // The peer goes away while the stream waits for credit it never grants.
    let Download {
        inbound,
        mut output,
    } = cancelled;
    drop(inbound);
    loop {
        let (_, next) = timeout(Duration::from_secs(3), output.recv())
            .await
            .expect("output in time")
            .expect("open");
        if let Output::Closed(code) = next {
            assert_eq!(code, "cancelled");
            break;
        }
    }
}
