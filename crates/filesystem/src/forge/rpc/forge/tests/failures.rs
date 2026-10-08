use super::*;
use crate::forge::ports::forge::{CheckDetailsQuery, ForgeFailureKind};
use crate::support::error::ErrorCode;

#[derive(Debug)]
struct FailedForge(ForgeRuntimeError);

impl ForgeRuntime for FailedForge {
    fn search(
        &self,
        _: &str,
        _: &str,
        _: usize,
        _: &[ForgeSearchKind],
    ) -> Result<ForgeSearch, ForgeRuntimeError> {
        Err(self.0.clone())
    }
    fn create_pull_request(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: Option<&str>,
    ) -> Result<PullRequestCreated, ForgeRuntimeError> {
        Err(self.0.clone())
    }
    fn current_pull_request_status(
        &self,
        _: &str,
    ) -> Result<PullRequestStatusRead, ForgeRuntimeError> {
        Err(self.0.clone())
    }
    fn merge_current_pull_request(
        &self,
        _: &str,
        _: PullRequestMergeMethod,
    ) -> Result<(), ForgeRuntimeError> {
        Err(self.0.clone())
    }
    fn set_current_pull_request_auto_merge(
        &self,
        _: &str,
        _: bool,
        _: Option<PullRequestMergeMethod>,
    ) -> Result<(), ForgeRuntimeError> {
        Err(self.0.clone())
    }
    fn pull_request_timeline(
        &self,
        _: &str,
        _: u64,
        _: &str,
        _: &str,
    ) -> Result<PullRequestTimeline, ForgeRuntimeError> {
        Err(self.0.clone())
    }
    fn check_details(
        &self,
        _: &str,
        _: CheckDetailsQuery<'_>,
    ) -> Result<CheckDetails, ForgeRuntimeError> {
        Err(self.0.clone())
    }
}

#[test]
fn unavailable_forges_preserve_auth_state_and_never_report_successful_mutations() {
    for (kind, auth, code) in [
        (ForgeFailureKind::CliMissing, "cli_missing", "UNKNOWN"),
        (
            ForgeFailureKind::Unauthenticated,
            "unauthenticated",
            "UNKNOWN",
        ),
        (ForgeFailureKind::NoRemote, "no_remote", "UNKNOWN"),
        (ForgeFailureKind::NotGitRepository, "error", "NOT_GIT_REPO"),
        (ForgeFailureKind::NotAllowed, "error", "NOT_ALLOWED"),
        (ForgeFailureKind::MergeConflict, "error", "MERGE_CONFLICT"),
    ] {
        let forge = Forge::new(Box::new(FailedForge(ForgeRuntimeError {
            forge: None,
            kind,
            message: "adapter failed".to_owned(),
        })));
        for method in ["forge.search.request", "github.search.request"] {
            let result = execute(&forge, method, json!({"cwd":"/repo","query":"fix"})).unwrap();
            assert_eq!(result["items"], json!([]));
            assert_eq!(result["authState"], auth);
            assert_eq!(result["error"], "adapter failed");
        }
        let status = execute(&forge, "checkout.pr.status.request", json!({"cwd":"/repo"})).unwrap();
        assert_eq!(status["authState"], auth);
        assert!(status["status"].is_null());
        let timeline = execute(
            &forge,
            "checkout.pr.timeline.request",
            json!({"cwd":"/repo","prNumber":4,"repoOwner":"acme","repoName":"app"}),
        )
        .unwrap();
        assert_eq!(timeline["authState"], auth);
        assert_eq!(timeline["items"], json!([]));
        assert_eq!(timeline["error"]["message"], "adapter failed");
        for (method, params) in [
            (
                "checkout.pr.merge.request",
                json!({"cwd":"/repo","mergeMethod":"squash"}),
            ),
            (
                "checkout.forge.set_auto_merge.request",
                json!({"cwd":"/repo","enabled":true,"mergeMethod":"squash"}),
            ),
            (
                "checkout.forge.get_check_details.request",
                json!({"cwd":"/repo","checkRunId":1}),
            ),
        ] {
            let result = execute(&forge, method, params).unwrap();
            assert_eq!(result["success"], false);
            assert_eq!(result["error"]["code"], code);
            assert_eq!(result["error"]["message"], "adapter failed");
        }
    }
}

#[test]
fn invalid_forge_queries_are_rejected_before_adapter_calls() {
    for limit in [0, 51] {
        assert_eq!(
            execute(
                &forge(),
                "forge.search.request",
                json!({"cwd":"/repo","query":"","limit":limit})
            ),
            Err(ErrorCode::InvalidMessage)
        );
    }
    for fields in [
        json!({"repoOwner":"../escape"}),
        json!({"repoName":"bad/name"}),
        json!({"checkRunId":0}),
        json!({"workflowRunId":0}),
        json!({"changeRequestNumber":0}),
    ] {
        let mut params = json!({"cwd":"/repo"});
        params
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        assert_eq!(
            execute(&forge(), "checkout.forge.get_check_details.request", params),
            Err(ErrorCode::InvalidMessage)
        );
    }
    assert_eq!(
        execute(&forge(), "unknown", json!({})),
        Err(ErrorCode::MethodNotFound)
    );
}
