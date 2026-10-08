use std::os::unix::fs::PermissionsExt;

use super::*;

struct Fixture {
    root: tempfile::TempDir,
    repo: PathBuf,
    source: LocalWorkspaceRuntime,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = repository(root.path());
        git(
            &repo,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/acme/repo.git",
            ],
        );
        let executable = root.path().join("gh");
        fs::write(
            &executable,
            format!(
                r#"#!/bin/sh
if [ "$1 $2" = "pr view" ]; then
  printf 'read\n' >> '{root}/calls'
  response=$(cat '{root}/status.json')
  while [ -f '{root}/block' ]; do sleep 0.01; done
  if [ -f '{root}/fail' ]; then printf 'network unavailable\n' >&2; exit 1; fi
  printf '%s' "$response"
  exit 0
fi
exit 0
"#,
                root = root.path().display()
            ),
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        let source = LocalWorkspaceRuntime::new(
            LocalCheckout::new(root.path().join("managed")),
            LocalForge::with_executable(executable),
        );
        let fixture = Self { root, repo, source };
        fixture.status(136, "OPEN", "IN_PROGRESS", None);
        fixture
    }

    fn status(&self, number: u64, state: &str, check_status: &str, conclusion: Option<&str>) {
        fs::write(self.root.path().join("status.json"), serde_json::to_vec(&json!({
            "number":number,"url":format!("https://github.com/acme/repo/pull/{number}"),
            "title":"Workspace status","state":state,"isDraft":false,
            "baseRefName":"main","headRefName":"feature","mergeable":"MERGEABLE",
            "mergedAt":(state == "MERGED").then_some("2026-10-02T00:00:00Z"),
            "reviewDecision":"APPROVED","statusCheckRollup":[{
                "__typename":"CheckRun","name":"test","status":check_status,"conclusion":conclusion
            }]
        })).unwrap()).unwrap();
    }

    fn read(
        &self,
        predicate: impl Fn(&WorkspaceForgeSnapshot) -> bool,
    ) -> WorkspaceRuntimeSnapshot {
        wait_for(&self.source, &self.repo, |snapshot| {
            snapshot.forge.as_ref().is_some_and(&predicate)
        })
    }

    fn calls(&self) -> usize {
        fs::read_to_string(self.root.path().join("calls"))
            .unwrap_or_default()
            .lines()
            .count()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.root.path().join("block"));
    }
}

#[test]
fn workspace_runtime_forge_cache_updates_ci_and_merge_and_retains_success_on_network_failure() {
    let fixture = Fixture::new();
    let initial = fixture.read(|forge| forge.pull_request.is_some());
    assert_eq!(
        initial.forge.unwrap().pull_request.unwrap().checks_status,
        "pending"
    );
    for _ in 0..20 {
        fixture.source.snapshot(fixture.repo.to_str().unwrap());
    }
    assert_eq!(fixture.calls(), 1);

    fixture.status(136, "MERGED", "COMPLETED", Some("SUCCESS"));
    expire(&fixture.source, &fixture.repo, true);
    let merged = fixture.read(|forge| forge.pull_request.as_ref().is_some_and(|pr| pr.is_merged));
    let pr = merged.forge.unwrap().pull_request.unwrap();
    assert_eq!(pr.checks_status, "success");
    assert_eq!(pr.checks[0].status, "success");

    fs::write(fixture.root.path().join("fail"), "").unwrap();
    expire(&fixture.source, &fixture.repo, true);
    let failed = fixture.read(|forge| forge.error.is_some());
    assert_eq!(
        failed.forge.unwrap().pull_request.unwrap().number,
        Some(136)
    );
    assert!(failed.git.is_some());
}

#[test]
fn workspace_runtime_rejects_in_flight_pr_reads_after_branch_change() {
    let fixture = Fixture::new();
    fs::write(fixture.root.path().join("block"), "").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while fixture.calls() == 0 {
        fixture.source.snapshot(fixture.repo.to_str().unwrap());
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    git(&fixture.repo, &["checkout", "-b", "other"]);
    expire(&fixture.source, &fixture.repo, false);
    let switched = wait_for(&fixture.source, &fixture.repo, |snapshot| {
        snapshot
            .git
            .as_ref()
            .is_some_and(|git| git.current_branch.as_deref() == Some("other"))
    });
    assert!(switched.forge.is_none());
    fixture.status(42, "OPEN", "COMPLETED", Some("FAILURE"));
    fs::remove_file(fixture.root.path().join("block")).unwrap();
    let new_pr = fixture.read(|forge| {
        assert!(
            forge
                .pull_request
                .as_ref()
                .is_none_or(|pr| pr.number != Some(136))
        );
        forge.pull_request.is_some()
    });
    assert_eq!(new_pr.forge.unwrap().pull_request.unwrap().number, Some(42));
}

#[test]
fn workspace_runtime_missing_cli_keeps_local_git_information() {
    let root = tempfile::tempdir().unwrap();
    let repo = repository(root.path());
    git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/repo.git",
        ],
    );
    let source = LocalWorkspaceRuntime::new(
        LocalCheckout::new(root.path().join("managed")),
        LocalForge::with_executable(root.path().join("absent-gh")),
    );
    let result = wait_for(&source, &repo, |snapshot| snapshot.forge.is_some());
    assert!(result.git.is_some());
    assert!(!result.forge.unwrap().features_enabled);
}
