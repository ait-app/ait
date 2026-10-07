use super::*;
use std::io::{Read, Seek, SeekFrom};

#[tokio::test]
async fn acknowledged_single_connection_upload_retains_a_200_mib_document() {
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("log");
    let state = temp.path().join("state");
    let mut process = start(&state, &log);
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, &["connection.single.v1", "file.upload.request"]).await;
    let size = 200 * 1024 * 1024;
    send_request(
        &mut socket,
        "large",
        "file.upload.request",
        json!({"fileName":"large.pdf","mimeType":"application/pdf","size":size,"modifiedAt":"now"}),
    )
    .await;
    acknowledged_frame(&mut socket, begin(size)).await;
    let mut chunk = vec![42; 128 * 1024];
    chunk[..8].copy_from_slice(b"%PDF-1.4");
    for _ in 0..size / chunk.len() as u64 {
        acknowledged_frame(&mut socket, FileFrame::Chunk(chunk.clone())).await;
    }
    let mut end = file_transfer::encode("large", &FileFrame::End).unwrap();
    end[0] += 0x30;
    socket.send(Message::Binary(end.into())).await.unwrap();
    let reply = receive(&mut socket).await;
    assert_eq!(reply["result"]["file"]["size"], size);
    let path = reply["result"]["file"]["path"].as_str().unwrap();
    assert_eq!(fs::metadata(path).unwrap().len(), size);
    let mut file = fs::File::open(path).unwrap();
    let mut read = vec![0; chunk.len()];
    file.read_exact(&mut read).unwrap();
    assert_eq!(read, chunk);
    file.seek(SeekFrom::End(-i64::try_from(chunk.len()).unwrap()))
        .unwrap();
    file.read_exact(&mut read).unwrap();
    assert_eq!(read, chunk);
    assert_eq!(
        receive(&mut socket).await["method"],
        "connection.upload.ack"
    );
    socket.close(None).await.unwrap();
    terminate(&mut process).await;
}

async fn acknowledged_frame(socket: &mut Socket, frame: FileFrame) {
    let mut bytes = file_transfer::encode("large", &frame).unwrap();
    let opcode = bytes[0];
    bytes[0] += 0x30;
    socket.send(Message::Binary(bytes.into())).await.unwrap();
    let ack = receive(socket).await;
    assert_eq!(ack["method"], "connection.upload.ack", "{ack}");
    assert_eq!(ack["params"]["opcode"], opcode);
}
