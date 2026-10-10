use super::*;

use filesystem::files::protocol::file_transfer::{self, FileBegin, FileFrame};

#[tokio::test]
async fn single_connection_queues_a_workspace_read_burst_behind_a_busy_worker() {
    read_burst(10, false).await;
}

#[tokio::test]
async fn single_connection_rejects_an_excessive_burst_without_blocking_ping() {
    read_burst(32, true).await;
}

async fn read_burst(reads: usize, expect_exhaustion: bool) {
    let root = tempfile::tempdir().unwrap();
    let fixture = Fixture::with_services(Services {
        filesystem: Some(filesystem_service::service(root.path())),
        ..Services::default()
    })
    .await;
    let mut socket = fixture.socket().await;
    let mut greeting = hello();
    greeting["capabilities"] = json!(fixture.api.shared.info().capabilities);
    greeting["required_capabilities"] = json!(["connection.single.v1", "file.upload.request"]);
    send(&mut socket, greeting).await;
    receive(&mut socket).await;
    send(
        &mut socket,
        json!({"type":"request","request_id":"upload",
        "method":"file.upload.request","params":{"fileName":"data.bin",
        "mimeType":"application/octet-stream","size":3,"modifiedAt":"now"}}),
    )
    .await;
    let held = fixture
        .api
        .shared
        .jobs
        .clone()
        .acquire_owned()
        .await
        .unwrap();
    let frame = FileFrame::Begin(FileBegin {
        mime: "application/octet-stream".into(),
        size: 3,
        encoding: "binary".into(),
        modified_at: "now".into(),
        file_name: Some("data.bin".into()),
        revision: None,
    });
    let mut bytes = file_transfer::encode("upload", &frame).unwrap();
    bytes[0] += 0x30;
    socket.send(Message::Binary(bytes.into())).await.unwrap();
    for index in 0..reads {
        send(
            &mut socket,
            json!({"type":"request","request_id":format!("read-{index}"),
            "method":"fs.explorer.request","params":{"cwd":root.path(),"mode":"list"}}),
        )
        .await;
    }
    send(
        &mut socket,
        json!({"type":"request","request_id":"ping",
        "method":"connection.ping","params":{"nonce":"queued"}}),
    )
    .await;

    let mut rejected = 0;
    loop {
        let response = receive(&mut socket).await;
        if response["request_id"] == "ping" {
            assert_eq!(response["result"]["nonce"], "queued");
            break;
        }
        assert_eq!(response["code"], "resource_exhausted", "{response}");
        rejected += 1;
    }
    assert_eq!(rejected > 0, expect_exhaustion);
    drop(held);
    let ack = receive(&mut socket).await;
    assert_eq!(ack["method"], "connection.upload.ack");
    for index in 0..reads - rejected {
        let response = receive(&mut socket).await;
        assert_eq!(response["request_id"], format!("read-{index}"));
        assert_eq!(response["type"], "response", "{response}");
    }
    socket.close(None).await.unwrap();
    fixture.stop().await;
}
