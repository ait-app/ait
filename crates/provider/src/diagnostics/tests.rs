use super::*;

#[test]
fn environment_overrides_program_and_log_roots() {
    let evidence = HarnessEvidence::configured(|key| match key {
        "HOME" => Some("/fixture".into()),
        "AIT_SERVER_OPENCODE_BIN" => Some("/bin/custom-opencode".into()),
        "AIT_DIAGNOSTICS_OPENCODE_LOG_DIR" => Some("/logs/native".into()),
        _ => None,
    });
    assert_eq!(evidence.sources.len(), 5);
    assert_eq!(
        evidence.sources[0].program,
        Path::new("/bin/custom-opencode")
    );
    assert_eq!(
        evidence.sources[0].logs.as_deref(),
        Some(Path::new("/logs/native"))
    );
    assert_eq!(
        evidence.sources[1].logs.as_deref(),
        Some(Path::new("/fixture/.codex/log"))
    );
}

#[test]
fn window_includes_boundaries_offsets_and_excludes_unscoped_payloads() {
    let start = DateTime::parse_from_rfc3339("2026-10-08T07:26:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let end = start + chrono::Duration::minutes(2);
    let text = concat!(
        "INFO 2026-10-08T07:25:59 old\n",
        "ERROR 2026-10-08T15:26:00+08:00 first\n",
        "  private continuation\n",
        "{\"timestamp\":\"2026-10-08T07:27:00Z\",\"error\":\"failed\"}\n",
        "WARN 2026-10-08 07:28:00 last\n",
        "ERROR 2026-10-08T07:28:01 new\n",
    );
    let (report, skipped) = window_excerpt(text, start, end);
    assert!(report.contains("first"));
    assert!(report.contains("last"));
    assert!(report.contains("failed"));
    assert!(!report.contains("old"));
    assert!(!report.contains("new"));
    assert!(!report.contains("private"));
    assert_eq!(skipped, 3);
    assert!(timestamp("not a date").is_none());
    assert_eq!(
        timestamp("ERROR 2026-10-08 15:26:00+08:00 spaced offset"),
        Some(start)
    );
    assert!(timestamp("ERROR 2026-10-08T07:27:00+invalid malformed offset").is_none());
}

#[test]
fn file_reads_are_bounded_and_exclude_transcripts_and_symlinks() {
    let directory = tempfile::tempdir().unwrap();
    let now = Utc::now();
    std::fs::write(
        directory.path().join("native.log"),
        format!("{now:?} native failure\n"),
    )
    .unwrap();
    std::fs::write(
        directory.path().join("transcript.jsonl"),
        format!("{now:?} private prompt\n"),
    )
    .unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        directory.path().join("native.log"),
        directory.path().join("alias.log"),
    )
    .unwrap();
    let report = collect_directory(directory.path(), now - chrono::Duration::minutes(1), now);
    assert!(report.contains("native failure"));
    assert!(!report.contains("private prompt"));
    assert!(!report.contains("alias.log"));
    assert!(collect_directory(&directory.path().join("missing"), now, now).contains("unavailable"));
    let path = directory.path().join("large.log");
    std::fs::write(
        &path,
        format!("{}\n{now:?} final line\n", "x".repeat(100_000)),
    )
    .unwrap();
    let (tail, truncated) = read_tail(&path).unwrap();
    assert!(truncated);
    assert!(tail.len() <= usize::try_from(FILE_BYTES).unwrap());
    assert!(!tail.contains('x'));
    assert!(tail.contains("final line"));
}

#[test]
fn probes_fail_without_hanging_and_collection_reports_missing_sources() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        assert!(
            probe(Path::new("/missing/ait-test-binary"), &["--version"])
                .await
                .is_err()
        );
        let evidence = HarnessEvidence {
            sources: vec![Source {
                name: "fixture",
                program: "/missing/ait-test-binary".into(),
                logs: None,
            }],
        };
        let report = evidence.collect(Utc::now(), Utc::now()).await;
        assert!(report.contains("Version: unavailable"));
        assert!(report.contains("no native log directory"));
        #[cfg(unix)]
        {
            assert_eq!(
                probe(Path::new("/bin/echo"), &["v2.0.20"]).await.unwrap(),
                "v2.0.20"
            );
            assert_eq!(
                probe(Path::new("/bin/sleep"), &["10"]).await,
                Err(ProbeError::Timeout)
            );
            assert_eq!(
                probe(Path::new("/bin/false"), &[]).await,
                Err(ProbeError::Failed)
            );
            assert_eq!(
                probe(Path::new("/bin/echo"), &[]).await,
                Err(ProbeError::InvalidOutput)
            );
            assert_eq!(
                probe(Path::new("/bin/echo"), &[&"x".repeat(5000)]).await,
                Err(ProbeError::TooLarge)
            );
        }
    });
}
