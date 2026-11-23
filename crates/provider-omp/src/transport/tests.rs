use super::*;

async fn read(input: &[u8], limit: usize) -> (Ending, Vec<Value>) {
    let (sender, mut receiver) = mpsc::channel(64);
    let ending = read_frames(input, limit, &sender).await;
    drop(sender);
    let mut frames = Vec::new();
    while let Some(frame) = receiver.recv().await {
        frames.push(frame.value);
    }
    (ending, frames)
}

#[tokio::test]
async fn invalid_lines_are_skipped_and_the_stream_continues() {
    let input = b"{broken\n\n[1]\r\n{\"content\":1}\n{\"type\":\"rpc_chunk\"}\n{\"type\":\"a\"}\r\n{\"type\":\"b\"}\n";

    let (ending, frames) = read(input, MAX_LINE).await;

    assert_eq!(ending, Ending::Closed);
    assert_eq!(frames, [json!({"type":"a"}), json!({"type":"b"})]);
}

#[tokio::test]
async fn a_line_over_the_limit_is_fatal_after_earlier_frames() {
    let line = r#"{"type":"a"}"#;
    let twice = format!("{line}\n{line}\n");

    let (ending, frames) = read(twice.as_bytes(), line.len()).await;
    assert_eq!(ending, Ending::Closed);
    assert_eq!(frames.len(), 2);

    let (ending, frames) = read(twice.as_bytes(), line.len() - 1).await;
    assert_eq!(ending, Ending::Oversized);
    assert!(frames.is_empty());

    let longer = format!("{line}\n{{\"type\":\"long\"}}\n");
    let (ending, frames) = read(longer.as_bytes(), line.len()).await;
    assert_eq!(ending, Ending::Oversized);
    assert_eq!(frames, [json!({"type":"a"})]);
}

#[tokio::test]
async fn output_ending_inside_a_line_is_incomplete() {
    let (ending, frames) = read(b"{\"type\":\"a\"}\n{\"type\":", MAX_LINE).await;

    assert_eq!(ending, Ending::Incomplete);
    assert_eq!(frames, [json!({"type":"a"})]);
}

#[tokio::test]
async fn a_closed_receiver_abandons_reading() {
    let (sender, receiver) = mpsc::channel(1);
    drop(receiver);

    assert_eq!(
        read_frames(b"{\"type\":\"a\"}\n".as_slice(), MAX_LINE, &sender).await,
        Ending::Abandoned
    );
}

#[test]
fn the_stderr_tail_keeps_only_the_newest_whole_lines() {
    let mut tail = Tail::default();
    tail.push(b"first line\nsecond line\n");
    assert!(!tail.truncated);
    assert_eq!(tail.excerpt().as_deref(), Some("first line\nsecond line"));

    tail.push(&[b'x'; STDERR_TAIL]);
    tail.push(b"\nold token=fixture");
    tail.push(&b"y".repeat(STDERR_EXCERPT));
    tail.push(b"\nnewest\n");

    assert!(tail.truncated);
    assert_eq!(tail.bytes.len(), STDERR_TAIL);
    // The partial first line and lines beyond the excerpt budget are omitted.
    let excerpt = tail.excerpt().unwrap();
    assert!(excerpt.ends_with("newest"));
    assert!(!excerpt.contains('x'));
    assert!(!excerpt.contains("fixture"));
}

#[test]
fn an_oversized_final_line_is_kept_whole_for_redaction() {
    let mut tail = Tail::default();
    let line = format!("token=secret {}", "z".repeat(STDERR_EXCERPT));
    tail.push(format!("earlier\n{line}\n").as_bytes());

    assert_eq!(tail.excerpt(), Some(line));
}

#[test]
fn a_tail_without_a_complete_line_has_no_excerpt() {
    let mut tail = Tail::default();
    tail.push(&[b'x'; STDERR_TAIL + 1]);

    assert_eq!(tail.excerpt(), None);
    assert_eq!(Tail::default().excerpt(), None);
}

#[cfg(unix)]
#[test]
fn exit_text_reports_codes_and_signals() {
    use std::os::unix::process::ExitStatusExt;

    assert!(exit_text(Some(ExitStatus::from_raw(3 << 8))).contains("code 3"));
    assert!(exit_text(Some(ExitStatus::from_raw(9))).contains("signal 9"));
    assert!(exit_text(None).contains("closed its output"));
}
