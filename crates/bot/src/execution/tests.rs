use domain::bot::{
    DeliveryStatus, GithubReviewContext, GithubReviewTrigger, Turn, TurnContext, TurnStatus,
};
use domain::{BotTurnId, ConversationId};

use super::review_schema;

fn turn(context: Option<TurnContext>) -> Turn {
    Turn {
        id: BotTurnId::generate(),
        conversation_id: ConversationId::generate(),
        message_id: "message".to_owned(),
        sequence: 1,
        generation: 0,
        text: "review".to_owned(),
        reply_to: None,
        context,
        reply: None,
        messages: Vec::new(),
        pending_permissions: Vec::new(),
        status: TurnStatus::Queued,
        delivery: DeliveryStatus::Pending,
        error: None,
        created_at: "2026-11-11T00:00:00Z".to_owned(),
    }
}

#[test]
fn github_review_turns_require_structured_findings() {
    assert!(review_schema(&turn(None), "codex").is_none());
    let context = TurnContext::GithubReview(GithubReviewContext {
        repository: "owner/repository".to_owned(),
        project_id: "project-1".to_owned(),
        pull_number: 7,
        title: "Change".to_owned(),
        url: "https://github.com/owner/repository/pull/7".to_owned(),
        base_ref: "main".to_owned(),
        head_ref: "feature".to_owned(),
        base_sha: "1".repeat(40),
        head_sha: "2".repeat(40),
        trigger: GithubReviewTrigger::ReviewerAssignment,
        trigger_user: "maintainer".to_owned(),
        trigger_comment_id: None,
        workspace_id: "workspace-review".to_owned(),
    });
    let review = turn(Some(context));
    assert!(review_schema(&review, "opencode").is_none());
    let schema = review_schema(&review, "codex").unwrap();
    assert_eq!(
        schema["required"],
        serde_json::json!(["summary", "findings"])
    );
    assert_eq!(
        schema["properties"]["findings"]["items"]["properties"]["severity"]["enum"],
        serde_json::json!(["high", "medium", "low"])
    );
}
