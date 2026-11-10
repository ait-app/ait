use serde_json::json;

use super::*;

#[test]
fn capabilities_merge_legacy_setup_and_script_names_into_canonical_methods() {
    assert_eq!(
        crate::automation::rpc::METHODS
            .iter()
            .map(|method| method.name)
            .collect::<Vec<_>>(),
        [
            "workspace.setup.status.request",
            "workspace.setup.summary.request",
            "workspace.setup.subscribe.request",
            "workspace.setup.run.request",
            "workspace.script.list.request",
            "workspace.script.start.request",
            "workspace.script.stop.request",
            "workspace.script.run.get.request",
            "workspace.script.run.wait.request"
        ]
    );
}

#[test]
fn requests_use_paseo_camel_case_fields_and_strip_unknown_fields() {
    let setup: WorkspaceSetupRequest = serde_json::from_value(json!({
        "workspaceId":"ws-1",
        "future":true
    }))
    .expect("setup request");
    assert_eq!(setup.workspace_id, "ws-1");

    let script: WorkspaceScriptRequest = serde_json::from_value(json!({
        "workspaceId":"ws-1",
        "scriptName":"web",
        "future":true
    }))
    .expect("script request");
    assert_eq!(script.script_name, "web");
}

#[test]
fn script_start_arguments_default_to_none_and_must_be_strings() {
    let start: WorkspaceScriptStartRequest =
        serde_json::from_value(json!({"workspaceId":"ws-1","scriptName":"verify"}))
            .expect("start request");
    assert!(start.args.is_empty());
    let start: WorkspaceScriptStartRequest = serde_json::from_value(
        json!({"workspaceId":"ws-1","scriptName":"verify","args":["--scope","all"]}),
    )
    .expect("start request with arguments");
    assert_eq!(start.args, ["--scope", "all"]);
    assert!(
        serde_json::from_value::<WorkspaceScriptStartRequest>(
            json!({"workspaceId":"ws-1","scriptName":"verify","args":"--scope all"})
        )
        .is_err()
    );
}

#[test]
fn script_run_results_use_camel_case_and_omit_timeout_outside_waits() {
    let run = WorkspaceScriptRun {
        run_id: "run_1".to_owned(),
        script_name: "verify".to_owned(),
        args: vec!["--scope".to_owned(), "all".to_owned()],
        commit: Some("abc".to_owned()),
        dirty: Some(false),
        started_at: "2026-11-10T00:00:00.000Z".to_owned(),
        finished_at: None,
        duration_ms: None,
        outcome: WorkspaceScriptRunOutcome::Running,
        exit_code: None,
        output: None,
        output_bytes: 12,
        truncated: false,
    };
    let read = serde_json::to_value(WorkspaceScriptRunResult {
        workspace_id: "ws-1".to_owned(),
        run: Some(run.clone()),
        timed_out: None,
        error: None,
    })
    .expect("read result");
    assert!(read.get("timedOut").is_none());
    assert_eq!(read["run"]["outcome"], "running");
    assert_eq!(read["run"]["outputBytes"], 12);
    assert!(read["run"]["output"].is_null());
    let waited = serde_json::to_value(WorkspaceScriptRunResult {
        workspace_id: "ws-1".to_owned(),
        run: Some(run),
        timed_out: Some(true),
        error: None,
    })
    .expect("wait result");
    assert_eq!(waited["timedOut"], true);
}

#[test]
fn setup_snapshot_matches_paseo_worktree_detail_shape() {
    let mut snapshot = WorkspaceSetupSnapshot {
        revision: 1,
        status: WorkspaceSetupStatus::Completed,
        detail: WorkspaceSetupDetail {
            kind: "worktree_setup".to_owned(),
            worktree_path: "/repo/worktree".to_owned(),
            branch_name: "feature".to_owned(),
            log: "done".to_owned(),
            commands: vec![WorkspaceSetupCommand {
                index: 1,
                command: "echo done".to_owned(),
                cwd: "/repo/worktree".to_owned(),
                log: "done\n".to_owned(),
                status: WorkspaceSetupCommandStatus::Completed,
                exit_code: Some(0),
                duration_ms: Some(12),
            }],
            truncated: false,
        },
        error: None,
        blocked_source: None,
    };
    let value = serde_json::to_value(&snapshot).expect("snapshot");
    assert_eq!(value["status"], "completed");
    assert_eq!(value["detail"]["type"], "worktree_setup");
    assert!(value["detail"].get("truncated").is_none());

    snapshot.detail.truncated = true;
    let truncated = serde_json::to_value(&snapshot).expect("truncated snapshot");
    assert_eq!(truncated["detail"]["truncated"], true);
}

#[test]
fn script_payload_omits_retired_proxy_fields() {
    let value = serde_json::to_value(WorkspaceScript {
        script_name: "dev".to_owned(),
        kind: WorkspaceScriptType::Service,
        hostname: "dev".to_owned(),
        port: Some(3000),
        lifecycle: WorkspaceScriptLifecycle::Stopped,
        health: (),
        exit_code: None,
        terminal_id: None,
        run_id: Some("run_1".to_owned()),
        accepts_args: true,
    })
    .expect("script");
    assert_eq!(value["runId"], "run_1");
    assert_eq!(value["acceptsArgs"], true);
    assert_eq!(value["type"], "service");
    assert_eq!(value["lifecycle"], "stopped");
    assert!(value.get("proxyUrl").is_none());
    assert!(value.get("publicProxyUrl").is_none());
    assert!(value["exitCode"].is_null());
    assert!(value["terminalId"].is_null());
    assert!(value["health"].is_null());
    assert!(value.get("localProxyUrl").is_none());
}
