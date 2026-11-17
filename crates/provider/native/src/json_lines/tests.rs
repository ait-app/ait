use super::*;

async fn read_all(input: &[u8], limit: usize) -> (Vec<(Value, usize)>, End) {
    let mut lines = JsonLines::new(input, limit);
    let mut frames = Vec::new();
    loop {
        match lines.next().await {
            Ok(frame) => frames.push((frame.value, frame.size)),
            Err(end) => return (frames, end),
        }
    }
}

#[tokio::test]
async fn frames_skip_blank_lines_and_end_closed() {
    let (frames, end) = read_all(b"{\"a\":1}\n\n  \n[2]\n", 64).await;

    assert_eq!(
        frames,
        vec![(serde_json::json!({"a":1}), 8), (serde_json::json!([2]), 4)]
    );
    assert_eq!(end, End::Closed);
}

#[tokio::test]
async fn short_final_frame_without_newline_is_delivered_before_closing() {
    let (frames, end) = read_all(b"1\n{\"last\":true}", 64).await;

    assert_eq!(frames.len(), 2);
    assert_eq!(frames[1], (serde_json::json!({"last":true}), 13));
    assert_eq!(end, End::Closed);
}

#[tokio::test]
async fn trailing_blank_text_closes_the_stream() {
    let (frames, end) = read_all(b"1\n   ", 64).await;

    assert_eq!(frames.len(), 1);
    assert_eq!(end, End::Closed);
}

#[tokio::test]
async fn frame_filling_the_limit_without_newline_is_a_read_failure() {
    let (frames, end) = read_all(b"1\n12345678\n", 8).await;
    assert_eq!(frames.len(), 1);
    assert_eq!(end, End::Read);

    let (frames, end) = read_all(b"1234567\n", 8).await;
    assert_eq!(frames.len(), 1);
    assert_eq!(end, End::Closed);
}

#[tokio::test]
async fn invalid_json_is_a_decode_failure() {
    let (frames, end) = read_all(b"1\nnot json\n2\n", 64).await;

    assert_eq!(frames.len(), 1);
    assert_eq!(end, End::Decode);
}

#[tokio::test]
async fn empty_stream_is_closed() {
    assert_eq!(read_all(b"", 64).await, (Vec::new(), End::Closed));
}
