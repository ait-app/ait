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
