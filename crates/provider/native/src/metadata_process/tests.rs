use super::*;

#[tokio::test]
async fn missing_program_is_unavailable() {
    let error = output(&mut Command::new("/ait-missing-metadata-program"))
        .await
        .unwrap_err();
    assert_eq!(error, AgentSessionError::Unavailable);
}

#[cfg(unix)]
#[tokio::test]
async fn output_is_bounded_and_nonzero_status_is_rejected() {
    assert_eq!(
        output(Command::new("/bin/sh").args(["-c", "printf metadata"]))
            .await
            .unwrap(),
        "metadata"
    );
    assert!(
        output(Command::new("/bin/sh").args(["-c", "exit 1"]))
            .await
            .is_err()
    );
    assert!(
        output(Command::new("/bin/sh").args(["-c", "head -c 1048577 /dev/zero"]))
            .await
            .is_err()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn bounded_output_returns_unsuccessful_status_with_stdout_within_its_limit() {
    let (status, bytes) = bounded_output(
        Command::new("/bin/sh").args(["-c", "printf signed-out; exit 1"]),
        10,
    )
    .await
    .unwrap();
    assert!(!status.success());
    assert_eq!(bytes, b"signed-out");
    assert_eq!(
        bounded_output(Command::new("/bin/sh").args(["-c", "printf 12345"]), 4)
            .await
            .unwrap_err(),
        AgentSessionError::Failed
    );
}
