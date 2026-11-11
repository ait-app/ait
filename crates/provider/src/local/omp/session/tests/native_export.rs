//! Native HTML export: owned staging, confinement validation, cleanup and turn isolation.
use super::*;
use crate::local::omp::tests::{complete, fixture};
use crate::ports::native_html_export::{ExportedNativeHtml, NativeHtmlExportError};

fn staging(root: &std::path::Path) -> std::path::PathBuf {
    root.join("agent/ait-native-exports")
}

fn owned_output(root: &std::path::Path) -> std::path::PathBuf {
    let directory = staging(root);
    std::fs::create_dir_all(&directory).unwrap();
    let output = directory.join(format!("ait-native-{}.html", Uuid::new_v4()));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)
        .unwrap();
    output
}

async fn begin(session: &mut Session, root: &std::path::Path, output: &std::path::Path) {
    session
        .begin_native_html_export(output, &staging(root))
        .await
        .unwrap();
}

/// The native renderer requires a persisted journal; the fake peer only writes one after a
/// message, so seed a completed turn before exporting.
async fn seed(session: &mut Session, spec: &AgentSessionSpec) {
    session.start_turn("seed", &spec.config).await.unwrap();
    let _ = complete(session).await;
}

async fn export_event(session: &mut Session) -> AgentTurnEvent {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(event) = session.poll_turn().unwrap() {
                if matches!(event, AgentTurnEvent::NativeHtmlExport(_)) {
                    return event;
                }
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("native export event")
}

fn read_artifact(export: ExportedNativeHtml) -> (String, u64, String) {
    use std::io::Read;
    let mut bytes = Vec::new();
    let mut file = export.artifact.file;
    file.read_to_end(&mut bytes).unwrap();
    (
        String::from_utf8(bytes).unwrap(),
        export.artifact.size,
        export.artifact.file_name,
    )
}

#[tokio::test]
async fn native_export_preserves_bytes_and_leaves_the_journal_untouched() {
    let (root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    seed(&mut session, &spec).await;
    let journal =
        std::path::PathBuf::from(session.state["sessionFile"].as_str().unwrap().to_owned());
    let before = std::fs::read(&journal).unwrap();
    let output = owned_output(root.path());
    begin(&mut session, root.path(), &output).await;
    let AgentTurnEvent::NativeHtmlExport(Ok(export)) = export_event(&mut session).await else {
        panic!("expected a successful native export");
    };
    let (text, size, file_name) = read_artifact(export);
    assert!(text.contains("NATIVE_FIXTURE_MARKER_27"));
    assert!(text.contains("<script>"));
    assert_eq!(size as usize, text.len());
    assert!(file_name.starts_with("ait-native-"));
    assert!(file_name.ends_with(".html"));
    // The owned staging file was adopted and removed; the journal is byte-identical.
    assert!(!output.exists());
    assert_eq!(std::fs::read(&journal).unwrap(), before);
    assert!(staging(root.path()).read_dir().unwrap().next().is_none());
    // The conversation remains usable after the export.
    session.start_turn("after", &spec.config).await.unwrap();
    let events = complete(&mut session).await;
    assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
    session.close().await.unwrap();
}

#[tokio::test]
async fn native_export_runs_during_an_active_turn_without_interrupting_it() {
    let (root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    seed(&mut session, &spec).await;
    let turn = session
        .start_turn("export-hold", &spec.config)
        .await
        .unwrap();
    // Wait until the native prompt was admitted and the turn is running.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if session.phase == TurnPhase::Running {
                return;
            }
            let _ = session.poll_turn().unwrap();
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    let output = owned_output(root.path());
    begin(&mut session, root.path(), &output).await;
    let AgentTurnEvent::NativeHtmlExport(Ok(export)) = export_event(&mut session).await else {
        panic!("expected a successful native export during the active turn");
    };
    let (text, _, _) = read_artifact(export);
    assert!(text.contains("NATIVE_FIXTURE_MARKER_27"));
    assert_eq!(session.active.as_deref(), Some(turn.as_str()));
    assert!(!output.exists());
    // Release the held turn; it must still complete normally.
    std::fs::write(root.path().join("agent/release-export-hold"), b"1").unwrap();
    let events = complete(&mut session).await;
    assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
    session.close().await.unwrap();
}

#[tokio::test]
async fn native_rejection_fails_the_export_and_removes_only_the_owned_file() {
    let (root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    seed(&mut session, &spec).await;
    std::fs::write(root.path().join("agent/export-mode"), b"reject").unwrap();
    let output = owned_output(root.path());
    begin(&mut session, root.path(), &output).await;
    let AgentTurnEvent::NativeHtmlExport(Err(NativeHtmlExportError::Rejected)) =
        export_event(&mut session).await
    else {
        panic!("expected a rejected native export");
    };
    assert!(!output.exists());
    // The session itself stays usable.
    session.start_turn("after", &spec.config).await.unwrap();
    let events = complete(&mut session).await;
    assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
    session.close().await.unwrap();
}

#[tokio::test]
async fn a_foreign_confirmed_path_is_never_adopted() {
    let (root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    seed(&mut session, &spec).await;
    std::fs::write(root.path().join("agent/export-mode"), b"wrong-path").unwrap();
    let output = owned_output(root.path());
    begin(&mut session, root.path(), &output).await;
    let AgentTurnEvent::NativeHtmlExport(Err(NativeHtmlExportError::Rejected)) =
        export_event(&mut session).await
    else {
        panic!("expected rejection of a foreign path");
    };
    assert!(!output.exists());
    session.close().await.unwrap();
}

#[tokio::test]
async fn oversize_native_output_fails_explicitly_without_truncation() {
    let (root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    seed(&mut session, &spec).await;
    std::fs::write(root.path().join("agent/export-mode"), b"oversize").unwrap();
    let output = owned_output(root.path());
    begin(&mut session, root.path(), &output).await;
    let AgentTurnEvent::NativeHtmlExport(Err(NativeHtmlExportError::TooLarge)) =
        export_event(&mut session).await
    else {
        panic!("expected an explicit size failure");
    };
    assert!(!output.exists());
    session.close().await.unwrap();
}

#[tokio::test]
async fn a_symlinked_output_is_rejected_and_the_target_survives() {
    let (root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    seed(&mut session, &spec).await;
    std::fs::write(root.path().join("agent/export-mode"), b"symlink").unwrap();
    let output = owned_output(root.path());
    begin(&mut session, root.path(), &output).await;
    let AgentTurnEvent::NativeHtmlExport(Err(NativeHtmlExportError::Rejected)) =
        export_event(&mut session).await
    else {
        panic!("expected rejection of a symlinked output");
    };
    // Only the owned link is removed; the pre-existing target keeps its content.
    assert!(std::fs::symlink_metadata(&output).is_err());
    let target = root.path().join("agent/symlink-target.html");
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "SYMLINK_TARGET_CONTENT"
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn a_silent_native_export_expires_without_killing_the_session() {
    let (root, mut client, spec) = fixture();
    client.deadline = Duration::from_millis(300);
    let mut session = open(&client, &spec, None).await.unwrap();
    seed(&mut session, &spec).await;
    std::fs::write(root.path().join("agent/export-mode"), b"silent").unwrap();
    let output = owned_output(root.path());
    begin(&mut session, root.path(), &output).await;
    let AgentTurnEvent::NativeHtmlExport(Err(NativeHtmlExportError::Io)) =
        export_event(&mut session).await
    else {
        panic!("expected an expired native export");
    };
    assert!(!output.exists());
    session.close().await.unwrap();
}

#[tokio::test]
async fn a_late_file_created_after_timeout_is_still_cleaned() {
    let (root, mut client, spec) = fixture();
    client.deadline = Duration::from_millis(200);
    let mut session = open(&client, &spec, None).await.unwrap();
    seed(&mut session, &spec).await;
    // The peer stays silent past the deadline, then creates the file and answers late.
    std::fs::write(root.path().join("agent/export-mode"), b"late").unwrap();
    let output = owned_output(root.path());
    begin(&mut session, root.path(), &output).await;
    let AgentTurnEvent::NativeHtmlExport(Err(NativeHtmlExportError::Io)) =
        export_event(&mut session).await
    else {
        panic!("expected the export to expire first");
    };
    // Drive polling until the late response arrives. First wait for the fixture to actually
    // create the late file (so the test cannot pass before the delayed write), then prove the
    // adapter removes it and delivers no second export event.
    let late_marker = root.path().join("agent/late-file-written");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(event) = session.poll_turn().unwrap() {
                assert!(
                    !matches!(event, AgentTurnEvent::NativeHtmlExport(_)),
                    "a settled export must not deliver a second event"
                );
            }
            if late_marker.exists() && std::fs::symlink_metadata(&output).is_err() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("late owned file created then cleaned");
    assert!(staging(root.path()).read_dir().unwrap().next().is_none());
    session.close().await.unwrap();
}

#[tokio::test]
async fn cancelling_an_in_flight_export_cleans_the_owned_file() {
    let (root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    seed(&mut session, &spec).await;
    std::fs::write(root.path().join("agent/export-mode"), b"silent").unwrap();
    let output = owned_output(root.path());
    begin(&mut session, root.path(), &output).await;
    session.cancel_native_html_export();
    // Abandon removes the owned staging file immediately and retains the record for any
    // late response; closing must not leave it behind.
    assert!(!output.exists());
    session.close().await.unwrap();
    assert!(staging(root.path()).read_dir().unwrap().next().is_none());
}

#[tokio::test]
async fn an_unpersisted_session_cannot_start_a_native_export() {
    let (root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    seed(&mut session, &spec).await;
    // Remove the journal so the persisted-identity precondition fails.
    let journal =
        std::path::PathBuf::from(session.state["sessionFile"].as_str().unwrap().to_owned());
    std::fs::remove_file(&journal).unwrap();
    let output = owned_output(root.path());
    let result = session
        .begin_native_html_export(&output, &staging(root.path()))
        .await;
    assert!(matches!(result, Err(AgentSessionError::Rejected)));
    let _ = std::fs::remove_file(&output);
    session.close().await.unwrap();
}
