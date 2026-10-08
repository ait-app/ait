use super::*;

mod filesystem_service;

#[tokio::test]
async fn chunks_dispatch_once_after_assembly_in_legacy_and_single_connections() {
    for single in [false, true] {
        let fixture = Fixture::start().await;
        let mut socket = fixture.socket().await;
        let mut greeting = hello();
        if single {
            greeting["required_capabilities"] = json!(["connection.single.v1"]);
        }
        send(&mut socket, greeting).await;
        let info = receive(&mut socket).await;
        assert!(
            info["info"]["features"]
                .as_array()
                .unwrap()
                .contains(&json!("client-message-chunks-v1"))
        );
        let wire = json!({"type":"request","request_id":"large","method":"connection.ping","params":{"nonce":"complete"},"padding":"x".repeat(2 * 1024 * 1024)}).to_string();
        let bytes = wire.as_bytes();
        let mut offset = 0;
        for chunk in bytes.chunks(256 * 1024) {
            let mut frame = vec![0x30];
            frame.extend_from_slice(&7_u32.to_be_bytes());
            frame.extend_from_slice(&u32::try_from(bytes.len()).unwrap().to_be_bytes());
            frame.extend_from_slice(&u32::try_from(offset).unwrap().to_be_bytes());
            frame.extend_from_slice(chunk);
            socket.send(Message::Binary(frame.into())).await.unwrap();
            offset += chunk.len();
            let ack = receive(&mut socket).await;
            assert_eq!(ack["method"], "connection.chunk.ack");
            assert_eq!(ack["params"]["offset"], offset);
        }
        let reply = receive(&mut socket).await;
        assert_eq!(reply["request_id"], "large");
        assert_eq!(reply["result"]["nonce"], "complete");
        socket.close(None).await.unwrap();
        fixture.stop().await;
    }
}

#[tokio::test]
async fn acknowledged_upload_waits_for_contended_jobs_without_losing_the_file() {
    use filesystem::files::protocol::file_transfer::{self, FileBegin, FileFrame};
    let root = tempfile::tempdir().unwrap();
    let fixture = Fixture::with_services(Services {
        filesystem: Some(filesystem_service::service(root.path())),
        ..Services::default()
    })
    .await;
    let mut socket = fixture.socket().await;
    let mut greeting = hello();
    greeting["required_capabilities"] = json!(["connection.single.v1", "file.upload.request"]);
    send(&mut socket, greeting).await;
    receive(&mut socket).await;
    send(&mut socket, json!({"type":"request","request_id":"upload","method":"file.upload.request","params":{"fileName":"data.bin","mimeType":"application/octet-stream","size":3,"modifiedAt":"now"}})).await;
    let frames = [
        FileFrame::Begin(FileBegin {
            mime: "application/octet-stream".into(),
            size: 3,
            encoding: "binary".into(),
            modified_at: "now".into(),
            file_name: Some("data.bin".into()),
            revision: None,
        }),
        FileFrame::Chunk(vec![1, 2, 3]),
        FileFrame::End,
    ];
    for frame in frames {
        let held = fixture
            .api
            .shared
            .jobs
            .clone()
            .acquire_owned()
            .await
            .unwrap();
        let mut bytes = file_transfer::encode("upload", &frame).unwrap();
        let opcode = bytes[0];
        bytes[0] += 0x30;
        socket.send(Message::Binary(bytes.into())).await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(30), socket.next())
                .await
                .is_err()
        );
        drop(held);
        if opcode == 0x12 {
            let reply = receive(&mut socket).await;
            let path = reply["result"]["file"]["path"].as_str().unwrap();
            assert_eq!(std::fs::read(path).unwrap(), [1, 2, 3]);
        }
        let ack = receive(&mut socket).await;
        assert_eq!(ack["method"], "connection.upload.ack");
        assert_eq!(ack["params"]["opcode"], opcode);
    }
    socket.close(None).await.unwrap();
    fixture.stop().await;
}
