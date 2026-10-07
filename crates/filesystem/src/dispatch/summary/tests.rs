use super::*;
use crate::service::checkout::{DiffHunk, DiffLine, ParsedDiffFile};

#[test]
fn fills_only_missing_fields_and_preserves_explicit_wording() {
    let mut commit = json!({"message":"  "});
    apply(
        &mut commit,
        SummaryKind::CommitMessage,
        Some(&json!({"message":"Fix titles"})),
    );
    assert_eq!(commit["message"], "Fix titles");
    let mut pr = json!({"title":"My title","body":null});
    apply(
        &mut pr,
        SummaryKind::PullRequest,
        Some(&json!({"title":"Generated","body":"Generated body"})),
    );
    assert_eq!(pr, json!({"title":"My title","body":"Generated body"}));
    let mut missing = json!({"title":" ","body":"Explicit body"});
    apply(&mut missing, SummaryKind::PullRequest, None);
    assert_eq!(
        missing,
        json!({"title":"Update changes","body":"Explicit body"})
    );
    apply(&mut commit, SummaryKind::CommitMessage, None);
    assert_eq!(commit["message"], "Fix titles");
}

#[test]
fn bounded_diff_preserves_file_names_changes_and_unicode() {
    let diff = CheckoutDiff {
        diff_too_large: false,
        files: vec![ParsedDiffFile {
            path: "标题.rs".into(),
            old_path: None,
            is_new: true,
            is_deleted: false,
            additions: 1,
            deletions: 0,
            status: None,
            hunks: vec![DiffHunk {
                old_start: 0,
                old_count: 0,
                new_start: 1,
                new_count: 1,
                lines: vec![DiffLine {
                    kind: DiffLineKind::Add,
                    content: "新".repeat(100_000),
                    tokens: None,
                }],
            }],
        }],
    };
    let text = source(&diff, SummaryKind::CommitMessage);
    assert!(text.starts_with("Changed files:\nA 标题.rs (+1 -0)"));
    assert!(text.contains("+新"));
    assert!(text.len() <= 120_000);
}

#[derive(Debug)]
struct Generator {
    runtime: std::sync::Arc<model::Runtime>,
    requests: std::sync::Mutex<Vec<SummaryRequest>>,
    fail: bool,
}
impl model::summary::SummarySource for Generator {
    fn generate(&self, request: SummaryRequest) -> model::summary::SummaryFuture<'_> {
        Box::pin(async move {
            assert!(
                self.runtime.jobs.try_acquire().is_ok(),
                "generation must release foreground job admission"
            );
            self.requests.lock().unwrap().push(request);
            if self.fail {
                Err(model::summary::SummaryError::Unavailable)
            } else {
                Ok(
                    json!({"title":"Generated PR","body":"Generated details","message":"Generated commit"}),
                )
            }
        })
    }
    fn shutdown(&self) {}
}

fn state(root: &std::path::Path, fail: bool) -> (State, std::sync::Arc<Generator>) {
    use std::sync::{Arc, Mutex};
    let runtime = Arc::new(model::Runtime::new(model::ServerInfo {
        server_id: "test".into(),
        instance_id: "test".into(),
        version: None,
        listen: "127.0.0.1:0".into(),
        lifecycle: model::Lifecycle::Ready,
        protocol: model::server::VERSION,
        capabilities: vec![],
        implemented_capabilities: vec![],
        features: Vec::new(),
        limits: model::Limits::default(),
    }));
    let generator = Arc::new(Generator {
        runtime: runtime.clone(),
        requests: Mutex::new(Vec::new()),
        fail,
    });
    (
        State {
            summary_source: Some(generator.clone()),
            runtime,
            checkout: Some(Arc::new(Mutex::new(
                crate::service::checkout::Checkout::new(Box::new(
                    crate::local::checkout::LocalCheckout::new(root.join("managed")),
                )),
            ))),
            forge: None,
            files: None,
            skills: None,
            github_projects: None,
            worktrees: None,
            workspace_recovery: None,
            workspace_setup: None,
        },
        generator,
    )
}

fn git(root: &std::path::Path, args: &[&str]) {
    let result = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[tokio::test]
async fn missing_pr_fields_use_base_diff_without_holding_foreground_permit() {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-b", "main"]);
    git(root.path(), &["config", "user.email", "test@example.test"]);
    git(root.path(), &["config", "user.name", "Test"]);
    git(root.path(), &["commit", "--allow-empty", "-m", "Base"]);
    git(root.path(), &["checkout", "-b", "feature"]);
    std::fs::write(root.path().join("change.txt"), "committed behavior\n").unwrap();
    git(root.path(), &["add", "change.txt"]);
    git(root.path(), &["commit", "-m", "Change"]);
    let (state, generator) = state(root.path(), false);
    let mut params = json!({"cwd":root.path(),"title":"Explicit PR","baseRef":"main"});
    fill(&state, "checkout.pr.create.request", &mut params)
        .await
        .unwrap();
    assert_eq!(params["title"], "Explicit PR");
    assert_eq!(params["body"], "Generated details");
    {
        let requests = generator.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].kind, SummaryKind::PullRequest);
        assert!(requests[0].context.contains("committed behavior"));
    }
    fill(&state, "checkout.pr.create.request", &mut params)
        .await
        .unwrap();
    assert_eq!(generator.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn malformed_git_and_provider_failures_have_distinct_fallbacks() {
    let root = tempfile::tempdir().unwrap();
    let (state, generator) = state(root.path(), true);
    let mut params = json!({"cwd":root.path()});
    assert_eq!(
        fill(&state, "checkout.commit.request", &mut params).await,
        Err(ErrorCode::ProjectIo)
    );
    assert!(generator.requests.lock().unwrap().is_empty());
    assert_eq!(
        fill(
            &state,
            "checkout.commit.request",
            &mut json!({"message":23})
        )
        .await,
        Err(ErrorCode::InvalidMessage)
    );
    git(root.path(), &["init", "-b", "main"]);
    std::fs::write(root.path().join("new.txt"), "new file\n").unwrap();
    fill(&state, "checkout.commit.request", &mut params)
        .await
        .unwrap();
    assert_eq!(params["message"], "Update files");
    assert!(
        generator.requests.lock().unwrap()[0]
            .context
            .contains("new file")
    );
}
