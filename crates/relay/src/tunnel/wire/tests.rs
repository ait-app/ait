use super::*;
use futures_util::io::Cursor;

#[tokio::test]
async fn records_keep_binary_text_and_empty_control_boundaries() {
    let mut encoded = Cursor::new(Vec::new());
    write(&mut encoded, TEXT, "你好".as_bytes()).await.unwrap();
    write(&mut encoded, BINARY, &[0, 255, 1]).await.unwrap();
    write(&mut encoded, ACCEPT, &[]).await.unwrap();
    assert_eq!(&encoded.get_ref()[..5], &[4, 0, 0, 0, 6]);
    encoded.set_position(0);
    assert_eq!(
        read(&mut encoded).await.unwrap(),
        (TEXT, "你好".as_bytes().to_vec())
    );
    assert_eq!(read(&mut encoded).await.unwrap(), (BINARY, vec![0, 255, 1]));
    expect(&mut encoded, ACCEPT).await.unwrap();
}

#[tokio::test]
async fn invalid_kinds_sizes_and_utf8_are_rejected_before_body_allocation() {
    for bytes in [
        vec![99, 0, 0, 0, 0],
        vec![TEXT, 255, 255, 255, 255],
        vec![ACCEPT, 0, 0, 0, 1],
        vec![TEXT, 0, 0, 0, 1, 255],
    ] {
        assert!(read(&mut Cursor::new(bytes)).await.is_err());
    }
    assert!(
        write(&mut Cursor::new(Vec::new()), OPEN, &vec![0; 2049])
            .await
            .is_err()
    );
    assert!(
        write(&mut Cursor::new(Vec::new()), CLOSE, &[1])
            .await
            .is_err()
    );
}

#[test]
fn handshake_and_open_bind_the_control_epoch_and_visit() {
    let id = Uuid::nil();
    let hello = Prelude {
        protocol: PROTOCOL.into(),
        epoch: id,
        tunnel_id: id,
    };
    let json = serde_json::to_value(&hello).unwrap();
    assert_eq!(json["protocol"], PROTOCOL);
    assert_eq!(json["epoch"], id.to_string());
    let open = Open {
        relay_session_id: id,
        source_node_session_id: id,
        epoch: id,
        tunnel_id: id,
        mode: "ait-rust-single-v1".into(),
    };
    let mut encoded = serde_json::to_value(open).unwrap();
    assert!(serde_json::from_value::<Open>(encoded.clone()).is_ok());
    encoded["target_url"] = serde_json::json!("http://untrusted");
    assert!(serde_json::from_value::<Open>(encoded).is_err());
}
