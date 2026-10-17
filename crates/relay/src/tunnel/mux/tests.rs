use super::*;
use futures_util::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn slow_stream_does_not_block_another_stream_and_large_body_crosses_window() {
    let (left, right) = tokio::io::duplex(4096);
    let (requests, receiver) = mpsc::channel(16);
    let (incoming, mut streams) = mpsc::channel(16);
    let (_unused, unused) = mpsc::channel(1);
    let a = tokio::spawn(drive(left, yamux::Mode::Client, receiver, None));
    let b = tokio::spawn(drive(right, yamux::Mode::Server, unused, Some(incoming)));
    let (send, receive) = oneshot::channel();
    requests.send(send).await.unwrap();
    let mut slow = receive.await.unwrap();
    let upload = tokio::spawn(async move {
        slow.write_all(&vec![7; 1024 * 1024]).await.unwrap();
        slow.close().await.unwrap();
    });
    let mut slow_peer = timeout(Duration::from_secs(3), streams.recv())
        .await
        .unwrap()
        .unwrap();
    let (send, receive) = oneshot::channel();
    requests.send(send).await.unwrap();
    let mut fast = receive.await.unwrap();
    fast.write_all(b"ping").await.unwrap();
    fast.flush().await.unwrap();
    let mut fast_peer = timeout(Duration::from_secs(3), streams.recv())
        .await
        .unwrap()
        .unwrap();
    let mut ping = [0; 4];
    timeout(Duration::from_secs(3), fast_peer.read_exact(&mut ping))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&ping, b"ping");
    assert!(
        !upload.is_finished(),
        "slow stream should still be waiting for receive credit"
    );
    let mut body = vec![0; 1024 * 1024];
    timeout(Duration::from_secs(5), slow_peer.read_exact(&mut body))
        .await
        .unwrap()
        .unwrap();
    assert!(body.iter().all(|byte| *byte == 7));
    upload.await.unwrap();
    drop(slow_peer);
    fast_peer.write_all(b"pong").await.unwrap();
    fast_peer.flush().await.unwrap();
    timeout(Duration::from_secs(3), fast.read_exact(&mut ping))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&ping, b"pong");
    a.abort();
    b.abort();
}

#[tokio::test]
async fn byte_transport_preserves_chunks_and_responds_to_control_frames() {
    let (network, mut bytes) = tokio::io::duplex(1024);
    let (outgoing_tx, mut outgoing) = mpsc::channel(4);
    let (inbound, incoming_rx) = mpsc::channel(4);
    let sink = Box::pin(futures_util::sink::unfold(
        outgoing_tx,
        |sender, frame| async move {
            sender.send(frame).await.map_err(|_| invalid())?;
            Ok::<_, io::Error>(sender)
        },
    ));
    let stream = Box::pin(futures_util::stream::unfold(
        incoming_rx,
        |mut receiver| async move { receiver.recv().await.map(|frame| (frame, receiver)) },
    ));
    let task = tokio::spawn(transport(sink, stream, network));
    inbound
        .send(Ok(Frame::Binary(vec![0, 255, 9])))
        .await
        .unwrap();
    let mut body = [0; 3];
    tokio::io::AsyncReadExt::read_exact(&mut bytes, &mut body)
        .await
        .unwrap();
    assert_eq!(body, [0, 255, 9]);
    inbound.send(Ok(Frame::Ping(vec![42]))).await.unwrap();
    timeout(Duration::from_secs(3), async {
        loop {
            if let Some(Frame::Pong(body)) = outgoing.recv().await {
                assert_eq!(body, [42]);
                break;
            }
        }
    })
    .await
    .unwrap();
    inbound
        .send(Ok(Frame::Binary(vec![0; MAX_BLOCK + 1])))
        .await
        .unwrap();
    assert!(
        timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
}
