use super::*;

#[test]
fn spawn_guidance_retains_only_safe_os_facts_with_an_optional_code() {
    let error = VersionError::Spawn {
        kind: std::io::ErrorKind::PermissionDenied,
        os_error: Some(13),
    };
    assert_eq!(
        error.to_string(),
        "Pi executable could not be started (error kind: PermissionDenied, OS error: 13); check its interpreter, executable format, and execution permissions."
    );
    let error = VersionError::Spawn {
        kind: std::io::ErrorKind::PermissionDenied,
        os_error: None,
    };
    assert_eq!(
        error.to_string(),
        "Pi executable could not be started (error kind: PermissionDenied); check its interpreter, executable format, and execution permissions."
    );
}

#[tokio::test]
async fn writes_distinguish_closed_input_from_timeout_and_frame_limit() {
    let input = Mutex::new(None);
    assert_eq!(
        write(&input, &json!({}), Duration::from_secs(1)).await,
        Err(Category::Closed)
    );
    let _guard = input.lock().await;
    assert_eq!(
        write(&input, &json!({}), Duration::from_millis(1)).await,
        Err(Category::Timeout)
    );
    assert_eq!(
        write(
            &input,
            &json!("x".repeat(MAX_FRAME)),
            Duration::from_secs(1)
        )
        .await,
        Err(Category::Protocol)
    );
}

#[test]
fn absent_process_group_requires_no_cleanup() {
    signal_group(None, "-TERM");
    signal_group(None, "-KILL");
}

#[tokio::test]
async fn extension_notifications_are_ignored_but_unknown_methods_fail_explicitly() {
    let input = Mutex::new(None);
    for method in [
        "notify",
        "setStatus",
        "setWidget",
        "setTitle",
        "set_editor_text",
    ] {
        assert_eq!(
            extension_response(
                &input,
                &json!({"method":method,"id":"notice"}),
                Duration::from_secs(1)
            )
            .await,
            Ok(())
        );
    }
    for message in [
        json!({"method":"future_dialog","id":"request"}),
        json!({"method":"future_dialog"}),
        json!({}),
    ] {
        assert_eq!(
            extension_response(&input, &message, Duration::from_secs(1))
                .await
                .unwrap_err()
                .source,
            "extension_method"
        );
    }
    for method in ["select", "confirm", "input", "editor"] {
        assert_eq!(
            extension_response(&input, &json!({"method":method}), Duration::from_secs(1))
                .await
                .unwrap_err()
                .source,
            "extension_id"
        );
    }
}

#[test]
fn numeric_short_versions_are_actionable_and_probes_remain_bounded() {
    for text in ["1", "1.2", "1.2.3.4"] {
        let version = parse_version(text.as_bytes()).unwrap();
        assert_eq!(version.text, text);
        assert!(!version.supported);
    }
    assert!(parse_version(&[b'1'; MAX_VERSION_BYTES + 1]).is_err());
    assert!(parse_version(b"invalid").is_err());
}

#[cfg(unix)]
mod shutdown;
