//! Script run records, output capture, caller arguments and teardown at the process boundary.

use serde_json::json;

use super::*;

fn config(root: &Path, value: &Value) {
    fs::write(root.join("ait.json"), serde_json::to_vec(value).unwrap()).unwrap();
}

fn finished(
    automation: &LocalWorkspaceAutomation,
    workspace_id: &str,
    run_id: &str,
) -> ScriptRunSnapshot {
    let completion = automation
        .script_run_completion(workspace_id, run_id)
        .unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(5), completion.finished())
                .await
                .expect("script run finished");
        });
    automation
        .script_run(workspace_id, ScriptRunTarget::Run(run_id))
        .unwrap()
}

#[test]
fn runs_capture_combined_output_and_record_their_outcome() {
    let root = tempfile::tempdir().unwrap();
    config(
        root.path(),
        &json!({"scripts":{"check":{"command":"printf out; printf err >&2; exit 3"}}}),
    );
    let automation = automation();
    let workspace = placement(root.path(), "wks_output");
    let started = automation.start_script(&workspace, "check", &[]).unwrap();
    let run_id = started.run_id.clone().expect("run identity");
    let run = finished(&automation, "wks_output", &run_id);
    assert_eq!(run.outcome, ScriptRunOutcome::Failed);
    assert_eq!(run.exit_code, Some(3));
    assert_eq!(run.output, "outerr");
    assert_eq!(run.output_bytes, 6);
    assert!(!run.truncated);
    assert!(run.commit.is_none(), "a plain directory has no commit");
    assert!(run.duration_ms.is_some() && run.finished_at.is_some());
    let latest = automation
        .script_run("wks_output", ScriptRunTarget::LatestOf("check"))
        .unwrap();
    assert_eq!(latest.run_id, run_id);
    let listed = automation.list_scripts(&workspace).unwrap();
    assert_eq!(listed[0].run_id.as_deref(), Some(run_id.as_str()));
    assert_eq!(listed[0].exit_code, Some(3));
}

#[test]
fn arguments_reach_the_script_as_positional_parameters_without_shell_parsing() {
    let root = tempfile::tempdir().unwrap();
    config(
        root.path(),
        &json!({"scripts":{"echo":{"command":"printf '[%s]' \"$0\" \"$@\"","acceptsArgs":true}}}),
    );
    let automation = automation();
    let workspace = placement(root.path(), "wks_args");
    let hostile = [
        "--scope".to_owned(),
        "all; touch injected".to_owned(),
        "$(touch injected)".to_owned(),
        "`touch injected`".to_owned(),
        "a b".to_owned(),
    ];
    let started = automation
        .start_script(&workspace, "echo", &hostile)
        .unwrap();
    let run = finished(&automation, "wks_args", &started.run_id.unwrap());
    assert_eq!(run.outcome, ScriptRunOutcome::Succeeded);
    assert_eq!(
        run.output,
        "[echo][--scope][all; touch injected][$(touch injected)][`touch injected`][a b]"
    );
    assert_eq!(run.args, hostile);
    assert!(!root.path().join("injected").exists());
}

#[test]
fn scripts_without_accepts_args_refuse_arguments_before_starting() {
    let root = tempfile::tempdir().unwrap();
    config(
        root.path(),
        &json!({"scripts":{"plain":{"command":"touch started"},"open":{"command":"true","acceptsArgs":"yes"}}}),
    );
    let automation = automation();
    let workspace = placement(root.path(), "wks_refuse");
    for name in ["plain", "open"] {
        assert!(matches!(
            automation.start_script(&workspace, name, &["x".to_owned()]),
            Err(WorkspaceAutomationError::InvalidArguments(_))
        ));
    }
    assert!(
        automation
            .list_scripts(&workspace)
            .unwrap()
            .iter()
            .all(|script| !script.accepts_args && script.run_id.is_none())
    );
    assert!(!root.path().join("started").exists());
}

#[test]
fn replaced_runs_stay_readable_and_stopped_runs_are_marked_stopped() {
    let root = tempfile::tempdir().unwrap();
    config(
        root.path(),
        &json!({"scripts":{"once":{"command":"printf one"},"serve":{"type":"service","command":"while :; do sleep 1; done"}}}),
    );
    let automation = automation();
    let workspace = placement(root.path(), "wks_history");
    let first = automation.start_script(&workspace, "once", &[]).unwrap();
    let first_id = first.run_id.unwrap();
    finished(&automation, "wks_history", &first_id);
    let second = automation.start_script(&workspace, "once", &[]).unwrap();
    let second_id = second.run_id.unwrap();
    assert_ne!(first_id, second_id);
    let earlier = automation
        .script_run("wks_history", ScriptRunTarget::Run(&first_id))
        .unwrap();
    assert_eq!(earlier.outcome, ScriptRunOutcome::Succeeded);
    assert_eq!(earlier.output, "one");
    assert!(
        automation
            .script_run_completion("wks_history", &first_id)
            .is_ok()
    );

    let service = automation.start_script(&workspace, "serve", &[]).unwrap();
    automation.stop_script(&workspace, "serve").unwrap();
    let stopped = automation
        .script_run(
            "wks_history",
            ScriptRunTarget::Run(&service.run_id.unwrap()),
        )
        .unwrap();
    assert_eq!(stopped.outcome, ScriptRunOutcome::Stopped);

    for target in [
        ScriptRunTarget::Run("run_missing"),
        ScriptRunTarget::LatestOf("never"),
    ] {
        assert!(matches!(
            automation.script_run("wks_history", target),
            Err(WorkspaceAutomationError::UnknownRun(_))
        ));
    }
    assert!(matches!(
        automation.script_run("other", ScriptRunTarget::Run(&second_id)),
        Err(WorkspaceAutomationError::UnknownRun(_))
    ));
    automation
        .close_workspaces(&["wks_history".to_owned()])
        .unwrap();
    assert!(
        automation
            .script_run("wks_history", ScriptRunTarget::Run(&first_id))
            .is_err()
    );
}

#[test]
fn commands_receive_ait_and_legacy_placement_variables() {
    let root = tempfile::tempdir().unwrap();
    config(
        root.path(),
        &json!({"scripts":{"env":{"command":"printf '%s|%s|%s|%s' \"$AIT_BRANCH_NAME\" \"$PASEO_BRANCH_NAME\" \"$AIT_WORKTREE_PATH\" \"$AIT_WORKTREE_PORT\""}}}),
    );
    let automation = automation();
    let workspace = placement(root.path(), "wks_env");
    let started = automation.start_script(&workspace, "env", &[]).unwrap();
    let run = finished(&automation, "wks_env", &started.run_id.unwrap());
    let fields: Vec<_> = run.output.split('|').collect();
    assert_eq!(fields[0], "feature/test");
    assert_eq!(fields[1], "feature/test");
    assert_eq!(fields[2], workspace.worktree_path);
    assert!(fields[3].parse::<u16>().is_ok());
}

#[test]
fn teardown_runs_in_order_and_stops_at_the_first_failure() {
    let root = tempfile::tempdir().unwrap();
    let automation = automation();
    let workspace = placement(root.path(), "wks_teardown");
    assert!(automation.prepare_teardown(&workspace).unwrap().is_none());

    config(
        root.path(),
        &json!({"worktree":{"teardown":["printf '%s' \"$AIT_BRANCH_NAME\" > first.txt","exit 4","touch never.txt"]}}),
    );
    let failed = automation
        .prepare_teardown(&workspace)
        .unwrap()
        .expect("teardown configured")
        .run();
    assert!(
        matches!(failed, Err(WorkspaceAutomationError::Io(ref message)) if message == "Teardown command 2 failed with exit code 4"),
        "{failed:?}"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("first.txt")).unwrap(),
        "feature/test"
    );
    assert!(!root.path().join("never.txt").exists());

    config(
        root.path(),
        &json!({"worktree":{"teardown":"touch done.txt"}}),
    );
    automation
        .prepare_teardown(&workspace)
        .unwrap()
        .expect("teardown configured")
        .run()
        .unwrap();
    assert!(root.path().join("done.txt").exists());

    fs::write(root.path().join("ait.json"), "[").unwrap();
    assert!(matches!(
        automation.prepare_teardown(&workspace),
        Err(WorkspaceAutomationError::InvalidConfig(_))
    ));
}

#[test]
fn teardown_failures_to_start_name_the_command() {
    let root = tempfile::tempdir().unwrap();
    config(root.path(), &json!({"worktree":{"teardown":["true"]}}));
    let automation = automation();
    let job = automation
        .prepare_teardown(&placement(root.path(), "wks_gone"))
        .unwrap()
        .expect("teardown configured");
    drop(root);
    let result = job.run();
    assert!(
        matches!(result, Err(WorkspaceAutomationError::Io(ref message)) if message.starts_with("Teardown command 1 failed: ")),
        "{result:?}"
    );
}
