use super::*;
use std::time::Duration;

fn export(contents: &[u8]) -> Export {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("test.html");
    std::fs::write(&output, contents).unwrap();
    Export {
        id: "request".to_owned(),
        directory,
        output,
        file_name: "test.html".to_owned(),
        deadline: Instant::now() + Duration::from_secs(1),
        cancelled: tokio_util::sync::CancellationToken::new(),
        written: None,
    }
}

#[test]
fn adopts_native_html_verbatim_and_removes_staging_directory() {
    let request = export(b"<html><script>native()</script></html>");
    let path = request.output.clone();
    let mut exports = Exports {
        current: Some(request),
        ..Exports::default()
    };
    assert!(exports.response(
        &json!({"id":"request","command":"export_html", "success":true, "data":{"path":path}})
    ));
    let deadline = Instant::now() + Duration::from_secs(5);
    let artifact = loop {
        exports.expire();
        if let Some(result) = exports.take() {
            break result.unwrap();
        }
        assert!(Instant::now() < deadline, "native export did not settle");
        std::thread::yield_now();
    };
    let mut contents = String::new();
    let mut file = artifact.file;
    file.read_to_string(&mut contents).unwrap();
    assert_eq!(contents, "<html><script>native()</script></html>");
    assert!(!path.exists());
}

#[tokio::test]
async fn slow_copy_does_not_block_an_active_sessions_controls_or_cleanup_early() {
    use provider_core::ports::agent_session::AgentSession;
    let (_root, client, spec) = crate::tests::fixture();
    let mut session = super::super::open(&client, &spec, None).await.unwrap();
    session.start_turn("first", &spec.config).await.unwrap();
    crate::tests::complete(&mut session).await;
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    let request = export(b"native");
    let directory = request.directory.path().to_owned();
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    session.native_exports.start_adoption(
        request,
        Arc::new(tokio::sync::Semaphore::new(1)),
        move |export| {
            let _ = started.send(());
            wait.recv().unwrap();
            adopt(export)
        },
    );
    ready.await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        let _ = session.poll_turn().unwrap();
        session.cancel_turn(&turn).await.unwrap();
    })
    .await
    .expect("native controls must not wait for the file copy");
    session.cancel_native_html_export();
    assert!(directory.exists(), "the copying thread still owns staging");
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while directory.exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    session.close().await.unwrap();
}

#[tokio::test]
async fn cancelled_slow_copy_keeps_its_quota_until_io_finishes() {
    let budget = Arc::new(tokio::sync::Semaphore::new(1));
    let request = export(b"native");
    let directory = request.directory.path().to_owned();
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let mut first = Exports::default();
    first.start_adoption(request, Arc::clone(&budget), move |export| {
        let _ = started.send(());
        wait.recv().unwrap();
        adopt(export)
    });
    ready.await.unwrap();
    first.cancel();
    assert!(directory.exists());
    let mut second = Exports::default();
    second.start_adoption(export(b"second"), Arc::clone(&budget), adopt);
    assert!(matches!(
        second.take(),
        Some(Err(NativeHtmlExportError::Busy))
    ));
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while directory.exists() || budget.available_permits() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn closing_session_does_not_wait_for_slow_copy_or_delete_its_file_early() {
    use provider_core::ports::agent_session::AgentSession;
    let (_root, client, spec) = crate::tests::fixture();
    let mut session = super::super::open(&client, &spec, None).await.unwrap();
    let request = export(b"native");
    let directory = request.directory.path().to_owned();
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    session.native_exports.start_adoption(
        request,
        Arc::new(tokio::sync::Semaphore::new(1)),
        move |export| {
            let _ = started.send(());
            wait.recv().unwrap();
            adopt(export)
        },
    );
    ready.await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), session.close())
        .await
        .expect("session close must not wait for the file copy")
        .unwrap();
    assert!(matches!(
        session.take_native_html_export(),
        Some(Err(NativeHtmlExportError::Io))
    ));
    assert!(directory.exists());
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while directory.exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn timed_out_slow_copy_retains_staging_until_it_returns() {
    let mut request = export(b"native");
    request.deadline = Instant::now();
    let directory = request.directory.path().to_owned();
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let mut exports = Exports::default();
    exports.start_adoption(
        request,
        Arc::new(tokio::sync::Semaphore::new(1)),
        move |export| {
            let _ = started.send(());
            wait.recv().unwrap();
            adopt(export)
        },
    );
    ready.await.unwrap();
    exports.expire();
    assert!(matches!(
        exports.take(),
        Some(Err(NativeHtmlExportError::Io))
    ));
    assert!(directory.exists());
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while directory.exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[test]
fn rejects_wrong_path_and_keeps_unrelated_responses() {
    let request = export(b"hello");
    let mut exports = Exports {
        current: Some(request),
        ..Exports::default()
    };
    assert!(!exports.response(&json!({"id":"other"})));
    assert!(exports.response(
        &json!({"id":"request","command":"export_html","success":true,"data":{"path":"/other"}})
    ));
    assert!(matches!(
        exports.take(),
        Some(Err(NativeHtmlExportError::Rejected))
    ));
}

#[test]
fn timeout_retains_directory_until_late_response_then_cleans() {
    let mut request = export(b"initial");
    request.deadline = Instant::now();
    let path = request.output.clone();
    let mut exports = Exports {
        current: Some(request),
        ..Exports::default()
    };
    exports.expire();
    assert!(matches!(
        exports.take(),
        Some(Err(NativeHtmlExportError::Io))
    ));
    assert!(!path.exists());
    std::fs::write(&path, "late").unwrap();
    assert!(exports.response(&json!({"id":"request"})));
    assert!(!path.exists());
}

#[test]
fn cancel_discards_the_result_and_reaps_late_file_on_close() {
    let request = export(b"initial");
    let path = request.output.clone();
    let mut exports = Exports {
        current: Some(request),
        ..Exports::default()
    };
    exports.cancel();
    assert!(exports.take().is_none());
    assert!(!path.exists());
    std::fs::write(&path, "late").unwrap();
    exports.closed();
    assert!(!path.exists());
    assert!(exports.take().is_none());
}

#[test]
fn closing_with_pending_request_reports_failure() {
    let request = export(b"initial");
    let path = request.output.clone();
    let mut exports = Exports {
        current: Some(request),
        ..Exports::default()
    };
    exports.closed();
    assert!(!path.exists());
    assert!(matches!(
        exports.take(),
        Some(Err(NativeHtmlExportError::Io))
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn unconfirmed_close_retains_staging_until_exit_is_observed() {
    use std::os::unix::process::ExitStatusExt;
    let request = export(b"initial");
    let directory = request.directory.path().to_owned();
    let mut exports = Exports {
        current: Some(request),
        ..Exports::default()
    };
    let (sender, exit) = tokio::sync::watch::channel(None);
    exports.defer_cleanup(exit, None);
    assert!(directory.exists());
    assert!(matches!(
        exports.take(),
        Some(Err(NativeHtmlExportError::Io))
    ));
    sender.send_replace(Some(std::process::ExitStatus::from_raw(0)));
    tokio::time::timeout(Duration::from_secs(2), async {
        while directory.exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn unconfirmed_close_keeps_staging_when_exit_observer_disappears() {
    let request = export(b"initial");
    let directory = request.directory.path().to_owned();
    let mut exports = Exports {
        current: Some(request),
        ..Exports::default()
    };
    let (sender, exit) = tokio::sync::watch::channel(None);
    exports.defer_cleanup(exit, None);
    drop(sender);
    tokio::task::yield_now().await;
    assert!(directory.exists());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn cancellation_cleans_a_queued_request_only_after_unsent_is_confirmed() {
    let mut request = export(b"initial");
    let path = request.output.clone();
    let directory = request.directory.path().to_owned();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    request.written = Some(receiver);
    let cancel = request.cancelled.clone();
    let mut exports = Exports {
        current: Some(request),
        ..Exports::default()
    };
    exports.cancel();
    assert!(cancel.is_cancelled());
    assert!(directory.exists());
    assert!(!path.exists());
    sender.send(OptionalWrite::Unsent).unwrap();
    exports.expire();
    assert!(!directory.exists());
    assert!(exports.late.is_empty());
}

#[test]
fn cancellation_retains_a_sent_request_until_native_response() {
    let mut request = export(b"initial");
    let path = request.output.clone();
    let directory = request.directory.path().to_owned();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    request.written = Some(receiver);
    let mut exports = Exports {
        current: Some(request),
        ..Exports::default()
    };
    exports.cancel();
    sender.send(OptionalWrite::Sent).unwrap();
    exports.expire();
    assert!(directory.exists());
    std::fs::write(&path, "late").unwrap();
    assert!(exports.response(&json!({"id":"request"})));
    assert!(!directory.exists());
}

#[test]
fn oversized_artifact_is_rejected_without_truncation() {
    let request = export(b"");
    std::fs::OpenOptions::new()
        .write(true)
        .open(&request.output)
        .unwrap()
        .set_len(MAX_NATIVE_HTML_EXPORT_BYTES + 1)
        .unwrap();
    assert!(matches!(
        adopt(&request),
        Err(NativeHtmlExportError::TooLarge)
    ));
}

#[cfg(unix)]
#[test]
fn symlink_output_cannot_read_a_foreign_file() {
    let request = export(b"original");
    let foreign = tempfile::NamedTempFile::new().unwrap();
    std::fs::remove_file(&request.output).unwrap();
    std::os::unix::fs::symlink(foreign.path(), &request.output).unwrap();
    assert!(matches!(
        adopt(&request),
        Err(NativeHtmlExportError::Rejected)
    ));
    assert!(foreign.path().exists());
}
