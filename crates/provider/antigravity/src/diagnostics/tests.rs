use super::*;

#[test]
fn exposes_only_classified_messages_and_prefers_specific_failures() {
    for (text, expected) in [
        (
            "jetski: a command was auto-denied",
            Some(Failure::Permission),
        ),
        (
            "permission check failed: user denied permission",
            Some(Failure::Permission),
        ),
        (
            "AGY_ERROR: quota exceeded; token=fixture-secret",
            Some(Failure::Quota),
        ),
        ("authentication required", Some(Failure::Authentication)),
        ("AGY_ERROR: provider unavailable", Some(Failure::Native)),
        ("Authorization: Bearer fixture-secret", None),
    ] {
        assert_eq!(Failure::classify(text), expected);
    }
    let diagnostics = Diagnostics::default();
    diagnostics.observe(Failure::Permission);
    diagnostics.observe(Failure::Exit);
    assert_eq!(diagnostics.failure(), Some(Failure::Permission));
    assert!(!format!("{diagnostics:?}").contains("fixture-secret"));
    diagnostics.clear();
    assert_eq!(diagnostics.failure(), None);
}

#[tokio::test]
async fn drains_oversized_lines_and_classifies_a_following_notice_without_a_newline() {
    let mut stderr = vec![b'x'; 512 * 1024];
    stderr.extend_from_slice(b"\nAGY_ERROR: authentication failed; token=fixture-secret");
    let diagnostics = Diagnostics::default();
    diagnostics.drain(stderr.as_slice()).await;
    assert_eq!(diagnostics.failure(), Some(Failure::Authentication));
    assert!(
        !diagnostics
            .failure()
            .unwrap()
            .message()
            .contains("fixture-secret")
    );
}

#[tokio::test]
async fn accumulates_notices_split_across_pipe_reads() {
    use tokio::io::AsyncWriteExt;

    let (mut writer, reader) = tokio::io::duplex(8);
    let writing = tokio::spawn(async move {
        writer.write_all(b"jetski: a tool was auto-").await.unwrap();
        writer.write_all(b"denied\n").await.unwrap();
    });
    let diagnostics = Diagnostics::default();
    diagnostics.drain(reader).await;
    writing.await.unwrap();
    assert_eq!(diagnostics.failure(), Some(Failure::Permission));
}
