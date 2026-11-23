//! Wire budgets for the directory reads a client issues to open a Workspace.
//!
//! The suite builds a deterministic directory of `WORKSPACES` Workspaces with Git and pull
//! request facts, runs each read through `execute`, and checks the encoded JSON result bytes and
//! returned rows against `CEILINGS`. Raising a ceiling is a one-line change to that table; the
//! reason belongs in the PR.

use std::sync::Arc;

use domain::workspace::protocol::workspace::{
    CheckStatus, ChecksStatus, Mergeable, ReviewDecision,
};
use domain::workspace::runtime::{
    WorkspaceCheckSnapshot, WorkspaceDiffStat, WorkspaceForgeSnapshot, WorkspaceGitSnapshot,
    WorkspacePullRequestSnapshot, WorkspaceRuntimeSnapshot,
};
use model::workspace::runtime::WorkspaceRuntimeSource;
use serde_json::{Value, json};

use super::*;
use crate::directory::rpc::directory::execute;

/// Read name, maximum encoded result bytes, maximum returned rows.
///
/// Values are the measurement on 2026-11-23 plus about 25 percent, rounded up; row ceilings
/// that equal a fixture size are exact. The synchronized cold list ignores `page.limit` and
/// grows with the directory, so its ceiling is the measured value with no headroom.
const CEILINGS: &[(&str, usize, usize)] = &[
    ("directory: projects cold", 2_600, 10),
    ("directory: workspaces cold", 602_697, 200),
    ("directory: workspaces resume", 4_100, 1),
    ("directory: workspace target", 4_100, 1),
];

const PROJECTS: usize = 10;
const WORKSPACES: usize = 200;
/// Checks on every Workspace's pull request, a mid-sized CI matrix.
const CHECKS: usize = 24;

struct Measured {
    read: &'static str,
    bytes: usize,
    rows: usize,
}

impl Measured {
    /// Measure `value`; `rows` names its row array, or a single row object.
    fn new(read: &'static str, value: &Value, rows: &str) -> Self {
        Self {
            read,
            bytes: serde_json::to_vec(value).expect("encode the result").len(),
            rows: match &value[rows] {
                Value::Array(entries) => entries.len(),
                Value::Object(_) => 1,
                _ => 0,
            },
        }
    }
}

#[derive(Debug)]
struct Runtime;

impl WorkspaceRuntimeSource for Runtime {
    fn snapshot(&self, cwd: &str) -> WorkspaceRuntimeSnapshot {
        runtime(cwd)
    }
}

#[test]
fn workspace_open_reads_stay_within_their_wire_budgets() {
    let mut directory = populated().with_runtime_source(Arc::new(Runtime));

    let projects =
        execute(&mut directory, "project.list.request", json!({"sync":{}})).expect("project list");
    let cold = execute(
        &mut directory,
        "workspace.list.request",
        json!({"sort":[{"key":"activity_at","direction":"desc"}],"page":{"limit":WORKSPACES},
            "sync":{}}),
    )
    .expect("workspace list");
    directory
        .set_workspace_title("wks_007", Some("Renamed while the client was away"), "now")
        .expect("rename one workspace");
    let resumed = execute(
        &mut directory,
        "workspace.list.request",
        json!({"sort":[{"key":"activity_at","direction":"desc"}],"page":{"limit":WORKSPACES},
            "sync":{"generation":cold["sync"]["generation"],"afterSeq":cold["sync"]["headSeq"]}}),
    )
    .expect("workspace resume");
    let target = execute(
        &mut directory,
        "workspace.get.request",
        json!({"workspaceId":"wks_042"}),
    )
    .expect("workspace target");

    assert_eq!(
        projects["projects"].as_array().map(Vec::len),
        Some(PROJECTS)
    );
    assert_eq!(cold["entries"].as_array().map(Vec::len), Some(WORKSPACES));
    assert_eq!(resumed["entries"].as_array().map(Vec::len), Some(1));
    assert_eq!(target["state"], "ready");
    enforce(&[
        Measured::new("directory: projects cold", &projects, "projects"),
        Measured::new("directory: workspaces cold", &cold, "entries"),
        Measured::new("directory: workspaces resume", &resumed, "entries"),
        Measured::new("directory: workspace target", &target, "workspace"),
    ]);
}

fn enforce(measured: &[Measured]) {
    let mut failures = Vec::new();
    for reading in measured {
        let Some(&(_, max_bytes, max_rows)) =
            CEILINGS.iter().find(|(read, ..)| *read == reading.read)
        else {
            failures.push(format!("{}: no ceiling in CEILINGS", reading.read));
            continue;
        };
        // The table is the suite's report; CI keeps it in the test log.
        println!(
            "wire budget | {:<42} | bytes {:>7} / {:>7} | rows {:>3} / {:>3}",
            reading.read, reading.bytes, max_bytes, reading.rows, max_rows
        );
        if reading.bytes > max_bytes || reading.rows > max_rows {
            failures.push(format!(
                "{}: {} bytes, {} rows exceed {max_bytes} bytes, {max_rows} rows",
                reading.read, reading.bytes, reading.rows
            ));
        }
    }
    for (read, ..) in CEILINGS {
        if !measured.iter().any(|reading| reading.read == *read) {
            failures.push(format!("{read}: ceiling has no measurement"));
        }
    }
    assert!(
        failures.is_empty(),
        "wire budgets exceeded:\n{}",
        failures.join("\n")
    );
}

fn populated() -> Directory {
    let projects = (0..PROJECTS)
        .map(|index| {
            let mut record = project();
            record.project_id = format!("prj_{index:02}");
            record.root_path = format!("/src/repository-{index:02}");
            record.display_name = format!("repository-{index:02}");
            record
        })
        .collect();
    let workspaces = (0..WORKSPACES)
        .map(|index| {
            let project = index % PROJECTS;
            let branch = format!("feature/timeline-pagination-{index:03}");
            let cwd = format!("/src/repository-{project:02}/.worktrees/{index:03}");
            let mut record = workspace();
            record.workspace_id = format!("wks_{index:03}");
            record.project_id = format!("prj_{project:02}");
            record.kind = PersistedWorkspaceKind::Worktree;
            record.display_name.clone_from(&branch);
            record.title = Some(format!("Fix flaky timeline pagination {index}"));
            record.branch = Some(branch);
            record.worktree_root = Some(cwd.clone());
            record.base_branch = Some("main".to_owned());
            record.is_paseo_owned_worktree = true;
            record.main_repo_root = Some(format!("/src/repository-{project:02}"));
            record.cwd = cwd;
            record
        })
        .collect();
    Directory::new(DirectoryDependencies {
        projects: Box::new(Projects {
            records: Mutex::new(projects),
            unavailable: false,
        }),
        workspaces: Box::new(Workspaces(Mutex::new(workspaces))),
        source: Box::<Source>::default(),
        config_store: Box::<ConfigStore>::default(),
        icon_store: Box::<IconStore>::default(),
        server_id: "server-test".to_owned(),
    })
}

/// Git and pull request facts for the Workspace at `cwd`, with `CHECKS` checks.
fn runtime(cwd: &str) -> WorkspaceRuntimeSnapshot {
    let branch = cwd.rsplit('/').next().unwrap_or_default();
    WorkspaceRuntimeSnapshot {
        git: Some(WorkspaceGitSnapshot {
            current_branch: Some(format!("feature/timeline-pagination-{branch}")),
            remote_url: Some("https://github.com/acme/repository.git".to_owned()),
            is_managed_worktree: true,
            is_dirty: Some(true),
            ahead_behind: Some((3, 1)),
            ahead_of_origin: Some(2),
            behind_of_origin: Some(0),
            diff_stat: Some(WorkspaceDiffStat {
                additions: 420,
                deletions: 35,
            }),
        }),
        forge: Some(WorkspaceForgeSnapshot {
            features_enabled: true,
            forge: Some("github".to_owned()),
            pull_request: Some(WorkspacePullRequestSnapshot {
                number: Some(800),
                url: format!("https://github.com/acme/repository/pull/{branch}"),
                title: "Fix flaky timeline pagination".to_owned(),
                state: "open".to_owned(),
                base_ref_name: "main".to_owned(),
                head_ref_name: format!("feature/timeline-pagination-{branch}"),
                is_merged: false,
                is_draft: false,
                mergeable: Mergeable::Mergeable,
                checks: (0..CHECKS)
                    .map(|index| WorkspaceCheckSnapshot {
                        name: format!("test (ubuntu-latest, shard {index})"),
                        status: CheckStatus::Success,
                        url: Some(format!(
                            "https://github.com/acme/repository/actions/runs/1/job/{index}"
                        )),
                        workflow: Some("CI".to_owned()),
                        duration: Some("4m 12s".to_owned()),
                        traits: None,
                    })
                    .collect(),
                checks_status: ChecksStatus::Success,
                review_decision: Some(ReviewDecision::Approved),
                repo_owner: Some("acme".to_owned()),
                repo_name: Some("repository".to_owned()),
                github: None,
            }),
            error: None,
        }),
    }
}
