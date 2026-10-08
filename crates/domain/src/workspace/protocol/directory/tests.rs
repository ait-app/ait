use serde_json::json;

use super::{
    ProjectCreateDirectoryRequest, ProjectRenameRequest, WorkspaceCreateRequest,
    WorkspaceCreateSource, WorkspaceListRequest, WorkspacePinSetRequest, WorkspaceTitleSetRequest,
};

#[test]
fn parses_paseo_camel_case_mutation_payloads() {
    assert_eq!(
        serde_json::from_value::<ProjectRenameRequest>(json!({
            "projectId": "prj_1",
            "customName": null
        }))
        .unwrap(),
        ProjectRenameRequest {
            project_id: "prj_1".to_owned(),
            custom_name: None,
        }
    );
    assert_eq!(
        serde_json::from_value::<WorkspaceTitleSetRequest>(json!({
            "workspaceId": "wks_1",
            "title": "  next  "
        }))
        .unwrap()
        .title,
        Some("  next  ".to_owned())
    );
    assert!(
        serde_json::from_value::<WorkspacePinSetRequest>(json!({
            "workspaceId": "wks_1",
            "pinned": "yes"
        }))
        .is_err()
    );
}

#[test]
fn parses_paseo_project_and_workspace_creation_payloads() {
    let request: ProjectCreateDirectoryRequest = serde_json::from_value(json!({
        "parentPath": "/Users/example/dev",
        "name": "new-project"
    }))
    .unwrap();
    assert_eq!(request.name, "new-project");

    let request: WorkspaceCreateRequest = serde_json::from_value(json!({
        "workspaceId": "wks_0123456789abcdef",
        "idempotencyKey": "create-1",
        "title": "Review",
        "source": {"kind":"directory", "path":"/repo", "projectId":"prj_1"}
    }))
    .unwrap();
    assert!(matches!(
        request.source,
        WorkspaceCreateSource::Directory { project_id: Some(project), .. } if project == "prj_1"
    ));

    for invalid in [
        json!({"workspaceId":"wks_UPPERCASE000000", "source":{"kind":"directory","path":"/repo"}}),
        json!({"idempotencyKey":"", "source":{"kind":"directory","path":"/repo"}}),
        json!({"source":{"kind":"unknown","path":"/repo"}}),
    ] {
        assert!(serde_json::from_value::<WorkspaceCreateRequest>(invalid).is_err());
    }
}

#[test]
fn validates_workspace_page_shape_during_deserialization() {
    let request: WorkspaceListRequest = serde_json::from_value(json!({
        "filter": {"projectId": "prj_1", "idPrefix": "ignored"},
        "sort": [{"key": "name", "direction": "asc"}],
        "page": {"limit": 20, "cursor": "10"}
    }))
    .unwrap();
    assert_eq!(request.page.unwrap().limit, 20);
    assert_eq!(
        request.filter.unwrap().id_prefix.as_deref(),
        Some("ignored")
    );
}

#[test]
fn parses_first_agent_context_without_requiring_combined_agent_creation() {
    for context in [
        json!({}),
        json!({"prompt":"first message"}),
        json!({"attachments":[{"type":"text","text":"context"}]}),
        json!({"prompt":"first message","attachments":[]}),
    ] {
        let request: WorkspaceCreateRequest = serde_json::from_value(json!({
            "source":{"kind":"directory","path":"/repo"},
            "firstAgentContext":context
        }))
        .unwrap();
        let parsed = request.first_agent_context.unwrap();
        assert_eq!(parsed.prompt.as_deref(), context["prompt"].as_str());
        assert_eq!(
            parsed.attachments,
            context["attachments"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        );
        assert!(request.agent.is_none());
    }
}

#[test]
fn rejects_malformed_first_agent_context() {
    for context in [
        json!("prompt"),
        json!(42),
        json!({"prompt":42}),
        json!({"attachments":{}}),
    ] {
        assert!(
            serde_json::from_value::<WorkspaceCreateRequest>(json!({
                "source":{"kind":"directory","path":"/repo"},
                "firstAgentContext":context
            }))
            .is_err()
        );
    }
}

#[test]
fn parses_and_validates_worktree_creation_source() {
    let request: WorkspaceCreateRequest = serde_json::from_value(json!({
        "source": {"kind":"worktree", "projectId":"prj_1", "worktreeSlug":"review",
            "branchName":"feature/review", "baseBranch":"main", "refName":"develop",
            "action":"branch-off", "githubPrNumber":42}
    }))
    .unwrap();
    let WorkspaceCreateSource::Worktree(source) = request.source else {
        panic!("expected worktree source");
    };
    assert_eq!(source.project_id.as_deref(), Some("prj_1"));
    assert_eq!(source.branch_name.as_deref(), Some("feature/review"));
    assert_eq!(source.base_branch.as_deref(), Some("main"));
    assert_eq!(source.ref_name.as_deref(), Some("develop"));
    assert_eq!(source.github_pr_number.unwrap().get(), 42);
    for field in [
        json!({"action":"invalid"}),
        json!({"refName":""}),
        json!({"branchName":""}),
        json!({"githubPrNumber":0}),
        json!({"githubPrNumber":-1}),
        json!({"githubPrNumber":1.5}),
        json!({"cwd":42}),
    ] {
        let mut source = field;
        source["kind"] = json!("worktree");
        assert!(
            serde_json::from_value::<WorkspaceCreateRequest>(json!({"source":source})).is_err()
        );
    }
}
