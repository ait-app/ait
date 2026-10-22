use super::*;
use serde_json::json;

#[test]
fn reassembles_utf8_across_chunk_boundaries_without_content_loss() {
    let value = json!({"type":"message_end","text":"中文\u{2028}text"});
    let bytes = serde_json::to_vec(&value).unwrap();
    let pieces: Vec<_> = bytes.chunks(7).collect();
    let mut decoder = Decoder::default();
    for (index, piece) in pieces.iter().enumerate() {
        let frame = json!({"type":"rpc_chunk","chunkId":"one","index":index,"count":pieces.len(),"byteLength":bytes.len(),"data":base64::engine::general_purpose::STANDARD.encode(piece)});
        let result = decoder.push(frame).unwrap();
        if index + 1 == pieces.len() {
            assert_eq!(result, Some(value.clone()));
        } else {
            assert!(result.is_none());
        }
    }
}

#[test]
fn rejects_oversized_interrupted_reordered_and_invalid_chunks() {
    let chunk =
        json!({"type":"rpc_chunk","chunkId":"id","index":0,"count":2,"byteLength":4,"data":"e30="});
    let mut decoder = Decoder::default();
    assert!(decoder.push(chunk.clone()).unwrap().is_none());
    assert!(decoder.push(json!({"type":"response"})).is_err());
    let mut decoder = Decoder::default();
    let mut invalid = chunk.clone();
    invalid["index"] = json!(1);
    assert!(decoder.push(invalid).is_err());
    let mut decoder = Decoder::default();
    let mut invalid = chunk.clone();
    invalid["byteLength"] = json!(MAX_LOGICAL + 1);
    assert!(decoder.push(invalid).is_err());
    let mut decoder = Decoder::default();
    let mut invalid = chunk.clone();
    invalid["data"] = json!("invalid");
    assert!(decoder.push(invalid).is_err());
    let mut decoder = Decoder::default();
    assert!(decoder.push(chunk.clone()).unwrap().is_none());
    assert!(decoder.push(chunk).is_err());
}
