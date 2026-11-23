use super::*;

#[tokio::test]
async fn optional_write_under_full_pipe_times_out_without_corrupting_stream() {
    let (writer, mut reader) = tokio::io::duplex(1);
    let writer = Mutex::new(writer);
    writer.lock().await.write_all(b"x").await.unwrap();
    let cancel = tokio_util::sync::CancellationToken::new();
    assert_eq!(
        write_optional(
            &writer,
            b"export\n",
            &cancel,
            Duration::from_millis(20),
            Duration::from_millis(20)
        )
        .await,
        OptionalWrite::Unsent
    );
    let mut byte = [0];
    reader.read_exact(&mut byte).await.unwrap();
    assert_eq!(&byte, b"x");
    writer.lock().await.write_all(b"y").await.unwrap();
    reader.read_exact(&mut byte).await.unwrap();
    assert_eq!(&byte, b"y");
}

#[tokio::test]
async fn cancellation_before_writer_admission_never_sends_later() {
    let (writer, mut reader) = tokio::io::duplex(32);
    let writer = Arc::new(Mutex::new(writer));
    let guard = writer.lock().await;
    let cancel = tokio_util::sync::CancellationToken::new();
    let task_writer = Arc::clone(&writer);
    let task_cancel = cancel.clone();
    let task = tokio::spawn(async move {
        write_optional(
            &task_writer,
            b"export\n",
            &task_cancel,
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .await
    });
    cancel.cancel();
    assert_eq!(task.await.unwrap(), OptionalWrite::Unsent);
    drop(guard);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), reader.read_u8())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn partial_optional_frame_reports_transport_failure() {
    let (writer, _reader) = tokio::io::duplex(1);
    let writer = Mutex::new(writer);
    let cancel = tokio_util::sync::CancellationToken::new();
    assert_eq!(
        write_optional(
            &writer,
            b"export\n",
            &cancel,
            Duration::from_millis(20),
            Duration::from_millis(20)
        )
        .await,
        OptionalWrite::Failed
    );
}

#[tokio::test]
async fn cancellation_after_partial_frame_finishes_the_frame() {
    let (writer, mut reader) = tokio::io::duplex(1);
    let writer = Arc::new(Mutex::new(writer));
    let cancel = tokio_util::sync::CancellationToken::new();
    let task_writer = Arc::clone(&writer);
    let task_cancel = cancel.clone();
    let task = tokio::spawn(async move {
        write_optional(
            &task_writer,
            b"ab",
            &task_cancel,
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .await
    });
    let mut first = [0];
    reader.read_exact(&mut first).await.unwrap();
    assert_eq!(&first, b"a");
    cancel.cancel();
    let mut second = [0];
    reader.read_exact(&mut second).await.unwrap();
    assert_eq!(&second, b"b");
    assert_eq!(task.await.unwrap(), OptionalWrite::Sent);
}

#[cfg(unix)]
#[tokio::test]
async fn backpressured_optional_command_does_not_close_native_session() {
    let root = tempfile::tempdir().unwrap();
    let client = OmpClient::new("/bin/sh".into());
    let args = [
        "-c".to_owned(),
        r#"printf '{"type":"ready"}\n'; exec sleep 30"#.to_owned(),
    ];
    let mut transport = Transport::spawn(
        &client,
        root.path().to_str().unwrap(),
        &args,
        None,
        Timeouts::default(),
    )
    .unwrap();
    let _ready = tokio::time::timeout(Duration::from_secs(5), transport.messages.recv())
        .await
        .unwrap()
        .unwrap();
    let input = Arc::clone(transport.input.as_ref().unwrap());
    let guard = input.lock().await;
    let (_, written) = transport
        .queue_optional(
            "export_html",
            json!({"outputPath":"unused"}),
            tokio_util::sync::CancellationToken::new(),
        )
        .unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(2), written)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(outcome, OptionalWrite::Unsent);
    assert!(!transport.exited());
    assert!(!transport.poisoned.load(Ordering::Acquire));
    drop(guard);
    transport.send(&json!({"type":"next"})).await.unwrap();
    assert!(transport.poll().is_ok());
    transport.close().await.unwrap();
}

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

#[cfg(unix)]
#[tokio::test]
async fn close_retries_a_shutdown_cancelled_during_a_failed_write() {
    use std::os::unix::process::ExitStatusExt;

    let root = tempfile::tempdir().unwrap();
    let client = OmpClient::new("/bin/sh".into());
    let script = r#"
        trap '' TERM
        printf '{"type":"ready"}\n'
        exec sleep 30
    "#;
    let args = ["-c".to_owned(), script.to_owned()];
    let mut transport = Transport::spawn(
        &client,
        root.path().to_str().unwrap(),
        &args,
        None,
        Timeouts {
            request: Duration::from_millis(25),
            ..Timeouts::default()
        },
    )
    .unwrap();
    let ready = tokio::time::timeout(Duration::from_secs(5), transport.messages.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ready.value["type"], "ready");

    let abort = json!({"type":"abort"});
    let closing = Arc::clone(&transport.closing);
    {
        // Stall the abort write until the transport's own request timeout starts shutdown.
        let input = Arc::clone(transport.input.as_ref().unwrap());
        let _blocked = input.lock().await;
        let mut write = std::pin::pin!(transport.send(&abort));
        tokio::time::timeout(
            Duration::from_secs(5),
            std::future::poll_fn(|cx| {
                assert!(write.as_mut().poll(cx).is_pending());
                if closing.load(Ordering::Acquire) {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            }),
        )
        .await
        .unwrap();
    }
    assert!(transport.exit.borrow().is_none());

    transport.close().await.unwrap();

    assert!(
        transport
            .exit
            .borrow()
            .is_some_and(|exit| exit.signal() == Some(9))
    );
}
