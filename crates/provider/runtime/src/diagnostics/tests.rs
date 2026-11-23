use super::*;

/// A source for the adapter whose variables use `key`, as the built-in adapters declare them.
fn declared(key: &str, program: &'static str, native_logs: NativeLogs) -> HarnessEvidenceSource {
    let (program_variable, log_directory_variable) = match key {
        "OPENCODE" => (
            "AIT_SERVER_OPENCODE_BIN",
            "AIT_DIAGNOSTICS_OPENCODE_LOG_DIR",
        ),
        "CODEX" => ("AIT_SERVER_CODEX_BIN", "AIT_DIAGNOSTICS_CODEX_LOG_DIR"),
        "CLAUDE" => ("AIT_SERVER_CLAUDE_BIN", "AIT_DIAGNOSTICS_CLAUDE_LOG_DIR"),
        _ => ("AIT_SERVER_PI_BIN", "AIT_DIAGNOSTICS_PI_LOG_DIR"),
    };
    HarnessEvidenceSource {
        program_variable,
        default_program: program,
        log_directory_variable,
        native_logs,
    }
}

#[test]
fn environment_overrides_program_and_log_roots() {
    let sources = [
        (
            "opencode",
            declared(
                "OPENCODE",
                "opencode",
                NativeLogs::ReportedByProgram(&["debug", "paths", "log"]),
            ),
        ),
        (
            "codex",
            declared(
                "CODEX",
                "codex",
                NativeLogs::UnderRoot {
                    root_variable: "CODEX_HOME",
                    home_directory: ".codex",
                    subdirectory: "log",
                },
            ),
        ),
        ("pi", declared("PI", "pi", NativeLogs::Unknown)),
    ];
    let evidence = HarnessEvidence::configured(sources, |key| match key {
        "HOME" => Some("/fixture".into()),
        "AIT_SERVER_PI_BIN" => Some("/bin/custom-pi".into()),
        "AIT_SERVER_OPENCODE_BIN" => Some("/bin/custom-opencode".into()),
        "AIT_DIAGNOSTICS_OPENCODE_LOG_DIR" => Some("/logs/native".into()),
        _ => None,
    });
    assert_eq!(evidence.sources.len(), 3);
    let pi = evidence
        .sources
        .iter()
        .find(|source| source.name == "pi")
        .unwrap();
    assert_eq!(pi.program, Path::new("/bin/custom-pi"));
    assert!(pi.logs.is_none());
    assert!(pi.log_query.is_none());
    assert_eq!(
        evidence.sources[0].program,
        Path::new("/bin/custom-opencode")
    );
    assert_eq!(
        evidence.sources[0].logs.as_deref(),
        Some(Path::new("/logs/native"))
    );
    assert_eq!(
        evidence.sources[0].log_query,
        Some(&["debug", "paths", "log"][..])
    );
    assert_eq!(
        evidence.sources[1].logs.as_deref(),
        Some(Path::new("/fixture/.codex/log"))
    );
    assert_eq!(evidence.sources[1].program, Path::new("codex"));
}

#[test]
fn root_variable_relocates_native_logs_and_unset_home_leaves_none() {
    let logs = NativeLogs::UnderRoot {
        root_variable: "CLAUDE_CONFIG_DIR",
        home_directory: ".claude",
        subdirectory: "debug",
    };
    let relocated =
        HarnessEvidence::configured([("claude", declared("CLAUDE", "claude", logs))], |key| {
            (key == "CLAUDE_CONFIG_DIR").then(|| "/config".into())
        });
    let homeless =
        HarnessEvidence::configured([("claude", declared("CLAUDE", "claude", logs))], |_| None);

    assert_eq!(
        relocated.sources[0].logs.as_deref(),
        Some(Path::new("/config/debug"))
    );
    assert!(homeless.sources[0].logs.is_none());
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
                name: "fixture".into(),
                program: "/missing/ait-test-binary".into(),
                logs: None,
                log_query: None,
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
                probe(Path::new("/usr/bin/false"), &[]).await,
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

#[cfg(unix)]
#[tokio::test]
async fn reported_log_directory_must_be_absolute() {
    const ABSOLUTE: &[&str] = &["/missing/ait-native-logs"];
    const RELATIVE: &[&str] = &["relative/logs"];
    let reported = |log_query: &'static [&'static str]| HarnessEvidence {
        sources: vec![Source {
            name: "fixture".into(),
            program: "/bin/echo".into(),
            logs: None,
            log_query: Some(log_query),
        }],
    };
    let now = Utc::now();

    let absolute = reported(ABSOLUTE).collect(now, now).await;
    let relative = reported(RELATIVE).collect(now, now).await;

    assert!(
        absolute.contains("Logs: directory unavailable"),
        "{absolute}"
    );
    assert!(
        relative.contains("Log discovery: non-absolute path rejected"),
        "{relative}"
    );
}
