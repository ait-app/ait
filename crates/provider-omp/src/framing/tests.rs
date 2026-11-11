use super::*;
use base64::engine::general_purpose::STANDARD;
use serde_json::json;

/// Serialize `value` padded past the v2 minimum and split it into `piece`-byte chunk lines.
fn chunked(id: &str, value: &Value, piece: usize) -> (Value, Vec<Vec<u8>>) {
    let mut value = value.clone();
    value["padding"] = json!("x".repeat(V2_FRAME_BYTES));
    let bytes = serde_json::to_vec(&value).unwrap();
    let pieces: Vec<_> = bytes.chunks(piece).collect();
    let lines = pieces
        .iter()
        .enumerate()
        .map(|(index, piece)| {
            serde_json::to_vec(&json!({"type":"rpc_chunk","chunkId":id,"index":index,
                "count":pieces.len(),"byteLength":bytes.len(),"data":STANDARD.encode(piece)}))
            .unwrap()
        })
        .collect();
    (value, lines)
}

fn chunk(index: usize, count: usize, length: usize, data: &str) -> Vec<u8> {
    serde_json::to_vec(
        &json!({"type":"rpc_chunk","chunkId":"id","index":index,"count":count,
        "byteLength":length,"data":data}),
    )
    .unwrap()
}

#[test]
fn reassembles_utf8_split_across_chunk_boundaries_without_content_loss() {
    let value = json!({"type":"message_end","text":"中文\u{2028}text"});
    let (value, lines) = chunked("one", &value, V2_CHUNK_BYTES - 1);
    let mut decoder = Decoder::default();

    let frames: Vec<_> = lines
        .iter()
        .map(|line| decoder.line(line).unwrap())
        .collect();

    assert!(frames[..frames.len() - 1].iter().all(Option::is_none));
    let frame = frames.last().unwrap().as_ref().unwrap();
    assert_eq!(frame.value, value);
    assert_eq!(frame.bytes, serde_json::to_vec(&value).unwrap().len());
}

#[test]
fn blank_carriage_return_and_untyped_lines_are_tolerated() {
    let mut decoder = Decoder::default();

    assert_eq!(decoder.line(b""), Ok(None));
    assert_eq!(decoder.line(b" \t\r"), Ok(None));
    assert_eq!(
        decoder.line(b"{\"type\":\"notice\"}\r").unwrap().unwrap(),
        Frame {
            value: json!({"type":"notice"}),
            bytes: 17
        }
    );
    assert_eq!(decoder.line(b"{broken"), Err(Problem::InvalidJson));
    assert_eq!(decoder.line(b"[1,2]"), Err(Problem::NotObject));
    assert_eq!(decoder.line(b"\"text\""), Err(Problem::NotObject));
    assert_eq!(decoder.line(b"{\"type\":7}"), Err(Problem::MissingType));
    assert_eq!(decoder.line(b"{\"content\":1}"), Err(Problem::MissingType));
    assert_eq!(
        decoder.line(b"{\"type\":\"next\"}").unwrap().unwrap().value["type"],
        "next"
    );
}

#[test]
fn chunk_headers_outside_protocol_v2_bounds_are_dropped() {
    let length = V2_FRAME_BYTES;
    let valid = json!({"type":"rpc_chunk","chunkId":"id","index":0,"count":5,
        "byteLength":length,"data":"e30="});
    for (key, value) in [
        ("chunkId", json!("")),
        ("chunkId", json!("x".repeat(MAX_CHUNK_ID + 1))),
        ("chunkId", json!(7)),
        ("index", json!(-1)),
        ("index", json!(5)),
        ("count", json!(1)),
        ("count", json!(V2_MAX_CHUNKS + 1)),
        ("byteLength", json!(V2_FRAME_BYTES - 1)),
        ("byteLength", json!(V2_REASSEMBLED_BYTES + 1)),
        ("data", json!("")),
        ("data", Value::Null),
    ] {
        let mut frame = valid.clone();
        frame[key] = value.clone();
        let line = serde_json::to_vec(&frame).unwrap();

        assert_eq!(
            Decoder::default().line(&line),
            Err(Problem::InvalidChunk),
            "{key}={value}"
        );
    }
    let mut frame = valid;
    // Sixty-four astral characters count as 128 UTF-16 code units, the largest valid id.
    frame["chunkId"] = json!("\u{1F600}".repeat(MAX_CHUNK_ID / 2));
    assert_eq!(
        Decoder::default().line(&serde_json::to_vec(&frame).unwrap()),
        Ok(None)
    );
}

#[test]
fn chunk_data_must_be_canonical_base64_within_the_chunk_size() {
    let length = V2_FRAME_BYTES;
    for data in [
        "e30".to_owned(),
        "e31=".to_owned(),
        "e3 0=".to_owned(),
        "e30-".to_owned(),
        STANDARD.encode(vec![b'x'; V2_CHUNK_BYTES + 1]),
    ] {
        assert_eq!(
            Decoder::default().line(&chunk(0, 5, length, &data)),
            Err(Problem::InvalidBase64),
            "{data:.16}"
        );
    }
    assert_eq!(
        Decoder::default().line(&chunk(
            0,
            5,
            length,
            &STANDARD.encode(vec![b'x'; V2_CHUNK_BYTES])
        )),
        Ok(None)
    );
}

#[test]
fn invalid_chunks_drop_the_open_sequence_without_stopping_the_stream() {
    let (value, lines) = chunked("one", &json!({"type":"message_end"}), V2_CHUNK_BYTES);
    let mut decoder = Decoder::default();
    assert_eq!(decoder.line(&lines[0]), Ok(None));

    assert_eq!(
        decoder.line(&chunk(1, 2, V2_FRAME_BYTES, "e30=")),
        Err(Problem::OutOfOrderChunk)
    );
    assert_eq!(decoder.line(&lines[1]), Err(Problem::OutOfOrderChunk));
    for line in &lines[..lines.len() - 1] {
        assert_eq!(decoder.line(line), Ok(None));
    }
    assert_eq!(
        decoder.line(lines.last().unwrap()).unwrap().unwrap().value,
        value
    );
}

#[test]
fn a_stray_continuation_or_repeated_chunk_is_reported_as_out_of_order() {
    let (_, lines) = chunked("one", &json!({"type":"message_end"}), V2_CHUNK_BYTES);
    let mut decoder = Decoder::default();

    assert_eq!(decoder.line(&lines[1]), Err(Problem::OutOfOrderChunk));
    assert_eq!(decoder.line(&lines[0]), Ok(None));
    assert_eq!(decoder.line(&lines[0]), Err(Problem::OutOfOrderChunk));
    assert_eq!(decoder.line(&lines[1]), Err(Problem::OutOfOrderChunk));
}

#[test]
fn frames_between_chunks_pass_through_without_breaking_the_sequence() {
    let (value, lines) = chunked("one", &json!({"type":"message_end"}), V2_CHUNK_BYTES);
    let mut decoder = Decoder::default();
    assert_eq!(decoder.line(&lines[0]), Ok(None));

    assert_eq!(
        decoder
            .line(b"{\"type\":\"notice\"}")
            .unwrap()
            .unwrap()
            .value,
        json!({"type":"notice"})
    );
    assert_eq!(decoder.line(b"{broken"), Err(Problem::InvalidJson));
    for line in &lines[1..lines.len() - 1] {
        assert_eq!(decoder.line(line), Ok(None));
    }
    assert_eq!(
        decoder.line(lines.last().unwrap()).unwrap().unwrap().value,
        value
    );
}

#[test]
fn reassembled_length_and_payload_must_match_the_declaration() {
    let length = V2_FRAME_BYTES;
    let mut decoder = Decoder::default();
    assert_eq!(decoder.line(&chunk(0, 2, length, "e30=")), Ok(None));
    assert_eq!(
        decoder.line(&chunk(1, 2, length, "e30=")),
        Err(Problem::ChunkLengthMismatch)
    );

    let full = STANDARD.encode(vec![b' '; V2_CHUNK_BYTES]);
    let mut decoder = Decoder::default();
    for index in 0..4 {
        assert_eq!(decoder.line(&chunk(index, 5, length, &full)), Ok(None));
    }
    assert_eq!(
        decoder.line(&chunk(4, 5, length, "e30=")),
        Err(Problem::ChunkLengthMismatch)
    );

    let payloads: [&[u8]; 4] = [
        b"[1]",
        b"{\"no\":\"type\"}",
        b"{\"type\":\"rpc_chunk\"}",
        b"{\"type\":\"x\",\"s\":\"\xff\"}",
    ];
    for payload in payloads {
        let mut bytes = vec![b' '; length - payload.len()];
        bytes.extend_from_slice(payload);
        let mut decoder = Decoder::default();
        let pieces: Vec<_> = bytes.chunks(V2_CHUNK_BYTES).collect();
        let mut result = Ok(None);
        for (index, piece) in pieces.iter().enumerate() {
            result = decoder.line(&chunk(index, pieces.len(), length, &STANDARD.encode(piece)));
        }
        assert_eq!(result, Err(Problem::InvalidChunkPayload));
    }
}

#[test]
fn protocol_v2_requires_the_advertised_frame_sizes() {
    let ready = json!({"type":"ready","protocolVersion":1,"supportedProtocolVersions":[1,2],
        "maxFrameBytes":V2_FRAME_BYTES,"maxReassembledFrameBytes":V2_REASSEMBLED_BYTES});
    assert!(supports_v2(&ready));
    for (key, value) in [
        ("type", json!("notice")),
        ("supportedProtocolVersions", json!([1])),
        ("supportedProtocolVersions", Value::Null),
        ("maxFrameBytes", json!(2 * V2_FRAME_BYTES)),
        ("maxFrameBytes", Value::Null),
        ("maxReassembledFrameBytes", json!(V2_FRAME_BYTES)),
    ] {
        let mut ready = ready.clone();
        ready[key] = value;
        assert!(!supports_v2(&ready), "{key}");
    }
}
