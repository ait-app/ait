use super::*;

#[test]
fn output_within_the_bound_is_kept_whole() {
    let mut buffer = OutputBuffer::default();
    buffer.push(b"first ");
    buffer.push(b"second");
    assert_eq!(buffer.render(), ("first second".to_owned(), 12, false));
}

#[test]
fn output_beyond_the_bound_keeps_its_head_and_most_recent_tail() {
    let mut buffer = OutputBuffer::default();
    buffer.push(&vec![b'h'; HEAD_BYTES]);
    buffer.push(&vec![b'x'; TAIL_BYTES]);
    buffer.push(b"end");
    let (output, total, truncated) = buffer.render();
    let written = HEAD_BYTES + TAIL_BYTES + 3;
    assert!(truncated);
    assert_eq!(total, u64::try_from(written).unwrap());
    assert_eq!(
        output.len(),
        HEAD_BYTES + TRUNCATION_MARKER.len() + TAIL_BYTES
    );
    assert!(output.starts_with(&"h".repeat(HEAD_BYTES)));
    assert!(output.ends_with("xend"));
    assert!(output.contains("script output truncated"));
}

#[test]
fn one_oversized_write_fills_head_and_tail_from_its_ends() {
    let mut buffer = OutputBuffer::default();
    let mut chunk = vec![b'a'; HEAD_BYTES];
    chunk.extend(vec![b'm'; TAIL_BYTES * 2]);
    chunk.extend(b"z");
    buffer.push(&chunk);
    let (output, _, truncated) = buffer.render();
    assert!(truncated);
    assert!(output.starts_with('a'));
    assert!(output.ends_with("mz"));
    assert_eq!(buffer.tail.len(), TAIL_BYTES);
}

#[test]
fn arguments_are_bounded_and_cannot_carry_nul() {
    validate_args(&["--scope".to_owned(), "all".to_owned()]).unwrap();
    validate_args(&[]).unwrap();
    for args in [
        vec![String::new(); MAX_ARGS + 1],
        vec!["x".repeat(MAX_ARG_BYTES + 1)],
        vec!["a\0b".to_owned()],
    ] {
        assert!(matches!(
            validate_args(&args),
            Err(WorkspaceAutomationError::InvalidArguments(_))
        ));
    }
}

#[test]
fn a_run_finishes_once_and_releases_waiters() {
    let output = Arc::new(Mutex::new(OutputBuffer::default()));
    let mut run = ScriptRun::start(
        "verify",
        &["--scope".to_owned()],
        CheckoutEvidence {
            commit: Some("abc".to_owned()),
            dirty: Some(true),
        },
        output.clone(),
    );
    let completion = run.completion();
    lock(&output).push(b"ok\n");
    let running = run.snapshot();
    assert_eq!(running.outcome, ScriptRunOutcome::Running);
    assert!(running.finished_at.is_none());
    assert_eq!(running.output, "ok\n");
    assert_eq!(running.commit.as_deref(), Some("abc"));
    assert_eq!(running.dirty, Some(true));
    assert_eq!(running.args, ["--scope"]);

    run.finish(Some(0), ScriptRunOutcome::Succeeded);
    run.finish(Some(1), ScriptRunOutcome::Stopped);
    let finished = run.snapshot();
    assert_eq!(finished.outcome, ScriptRunOutcome::Succeeded);
    assert_eq!(finished.exit_code, Some(0));
    assert!(finished.finished_at.is_some() && finished.duration_ms.is_some());
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(1), completion.finished())
                .await
                .expect("completion released");
        });
}

#[test]
fn history_keeps_only_the_most_recent_runs_per_workspace() {
    let mut history = HashMap::new();
    let mut ids = Vec::new();
    for _ in 0..=RUN_HISTORY {
        let run = ScriptRun::start("once", &[], CheckoutEvidence::default(), Arc::default());
        ids.push(run.id().to_owned());
        retain(&mut history, "wks", run);
    }
    retain(
        &mut history,
        "other",
        ScriptRun::start("once", &[], CheckoutEvidence::default(), Arc::default()),
    );
    let kept: Vec<_> = history["wks"]
        .iter()
        .map(|run| run.id().to_owned())
        .collect();
    assert_eq!(kept, ids[1..]);
    assert_eq!(history["other"].len(), 1);
}

#[test]
fn evidence_reads_head_and_dirty_state_of_git_checkouts_only() {
    let plain = tempfile::tempdir().unwrap();
    assert_eq!(
        checkout_evidence(&plain.path().to_string_lossy()),
        CheckoutEvidence::default()
    );
    let repository = tempfile::tempdir().unwrap();
    let cwd = repository.path().to_string_lossy().into_owned();
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(&cwd)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    std::fs::write(repository.path().join("file"), "one").unwrap();
    git(&["add", "file"]);
    git(&[
        "-c",
        "user.name=Ait",
        "-c",
        "user.email=ait@example.invalid",
        "commit",
        "-q",
        "-m",
        "initial",
    ]);
    let clean = checkout_evidence(&cwd);
    assert_eq!(clean.commit.as_ref().map(String::len), Some(40));
    assert_eq!(clean.dirty, Some(false));
    std::fs::write(repository.path().join("untracked"), "two").unwrap();
    assert_eq!(checkout_evidence(&cwd).dirty, Some(true));
}
