use std::sync::Arc;

use domain::ChannelId;
use domain::bot::{Channel, GithubRepositoryBinding, GithubReviewTrigger, TurnContext};
use hmac::{Hmac, Mac};
use secrecy::SecretString;
use serde_json::json;
use sha2::Sha256;

use crate::ports::Credentials;

use super::client::pem;
use super::{incoming, parse_report, review_command, review_context, trigger, verify_signature};

#[test]
fn mention_requires_the_bot_name_followed_by_review() {
    assert_eq!(
        review_command("please @review-bot review focus on auth", "review-bot"),
        Some("focus on auth".to_owned())
    );
    assert_eq!(review_command("@review-bot status", "review-bot"), None);
    assert_eq!(review_command("@other review", "review-bot"), None);
}

#[test]
fn webhook_signature_uses_constant_time_hmac_verification() {
    let secret = SecretString::from("\u{b}".repeat(20));
    let signature = "sha256=b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7";
    assert!(verify_signature(&secret, b"Hi There", signature).is_ok());
    assert!(verify_signature(&secret, b"Changed", signature).is_err());
}

#[test]
fn pem_parser_accepts_the_trailing_newline_from_github_keys() {
    let (_, der) = pem("-----BEGIN PRIVATE KEY-----\nAQID\n-----END PRIVATE KEY-----\n").unwrap();
    assert_eq!(der, vec![1, 2, 3]);
}

#[test]
fn review_report_accepts_plain_or_fenced_json() {
    let json = r#"{"summary":"Looks good","findings":[]}"#;
    assert_eq!(parse_report(json).unwrap().summary, "Looks good");
    assert_eq!(
        parse_report(&format!("```json\n{json}\n```"))
            .unwrap()
            .summary,
        "Looks good"
    );
}

#[test]
fn reviewer_assignment_only_targets_the_configured_app() {
    let payload = json!({
        "action":"review_requested",
        "requested_reviewer":{"login":"review-bot[bot]"}
    });
    let selected = trigger("pull_request", &payload, "review-bot")
        .unwrap()
        .unwrap();
    assert_eq!(selected.0, GithubReviewTrigger::ReviewerAssignment);
    assert!(
        trigger(
            "pull_request",
            &json!({"action":"review_requested","requested_reviewer":{"login":"someone"}}),
            "review-bot"
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn pull_request_assignee_assignment_targets_the_configured_app() {
    let payload = json!({
        "action":"assigned",
        "issue":{"pull_request":{}},
        "assignee":{"login":"review-bot[bot]"}
    });
    let selected = trigger("issues", &payload, "review-bot").unwrap().unwrap();
    assert_eq!(selected.0, GithubReviewTrigger::AssigneeAssignment);
}

#[test]
fn pull_request_assigned_event_targets_the_configured_app() {
    let assigned = |login: &str| {
        json!({
            "action":"assigned",
            "pull_request":{"number":7},
            "assignee":{"login":login}
        })
    };
    let selected = trigger("pull_request", &assigned("review-bot[bot]"), "review-bot")
        .unwrap()
        .unwrap();
    assert_eq!(selected.0, GithubReviewTrigger::AssigneeAssignment);
    assert!(
        trigger("pull_request", &assigned("someone"), "review-bot")
            .unwrap()
            .is_none()
    );
    // A plain issue's assignment is not a review request.
    let issue = json!({"action":"assigned","issue":{},"assignee":{"login":"review-bot[bot]"}});
    assert!(trigger("issues", &issue, "review-bot").unwrap().is_none());
}

#[derive(Debug)]
struct TestSecret;

impl Credentials for TestSecret {
    fn resolve(&self, _: &str) -> Result<SecretString, model::ErrorCode> {
        Ok(SecretString::from("webhook secret"))
    }
}

#[tokio::test]
async fn a_signed_push_observes_the_head_without_admitting_a_review() {
    let channel: Channel = serde_json::from_value(json!({
        "id": ChannelId::generate(),
        "botId": domain::BotId::generate(),
        "enabled": true,
        "config": {
            "kind":"github",
            "appId":"9",
            "installationId":"5",
            "mention":"review-bot",
            "privateKeyRef":"env:AIT_SERVER_CREDENTIAL_GITHUB",
            "webhookSecretRef":"env:AIT_SERVER_CREDENTIAL_WEBHOOK",
            "repositories":[{
                "fullName":"owner/repository",
                "projectId":"project-1",
                "allowedSenders":["maintainer"]
            }]
        }
    }))
    .unwrap();
    let before = "1".repeat(40);
    let after = "2".repeat(40);
    let payload = serde_json::to_vec(&json!({
        "action":"synchronize",
        "number":7,
        "before":before,
        "after":after,
        "installation":{"id":5},
        "repository":{"full_name":"Owner/Repository"},
        "sender":{"login":"contributor"}
    }))
    .unwrap();
    let mut mac = Hmac::<Sha256>::new_from_slice(b"webhook secret").unwrap();
    mac.update(&payload);
    let signature = format!("sha256={:x}", mac.finalize().into_bytes());
    let event = super::webhook(
        &channel,
        &(Arc::new(TestSecret) as Arc<dyn Credentials>),
        &reqwest::Client::new(),
        "pull_request",
        "delivery-1",
        &signature,
        &payload,
    )
    .await
    .unwrap();
    let Some(super::WebhookEvent::HeadChanged {
        key,
        before: old,
        after: new,
    }) = event
    else {
        panic!("a head update");
    };
    assert_eq!(
        (key.channel_id, key.repository.as_str(), key.pull_number),
        (channel.id, "owner/repository", 7)
    );
    assert_eq!((old, new), (before, after));
}

#[test]
fn same_pull_revision_has_one_stable_admission_identity() {
    let binding = GithubRepositoryBinding {
        full_name: "owner/repository".to_owned(),
        project_id: "project-1".to_owned(),
        allowed_senders: vec!["maintainer".to_owned()],
    };
    let pull = json!({
        "title":"Change",
        "html_url":"https://github.com/owner/repository/pull/7",
        "base":{"ref":"main","sha":"1111111111111111111111111111111111111111"},
        "head":{"ref":"feature","sha":"2222222222222222222222222222222222222222"}
    });
    let mention = review_context(
        &binding,
        "owner/repository",
        7,
        GithubReviewTrigger::Mention,
        "maintainer",
        Some("10".to_owned()),
        &pull,
    )
    .unwrap();
    let assigned = review_context(
        &binding,
        "owner/repository",
        7,
        GithubReviewTrigger::ReviewerAssignment,
        "maintainer",
        None,
        &pull,
    )
    .unwrap();
    let first = incoming(ChannelId::generate(), 5, mention, "focus");
    let second = incoming(first.channel_id, 5, assigned, "");
    assert_eq!(first.message_id, second.message_id);
    assert_eq!(first.thread, second.thread);
    assert!(matches!(first.context, Some(TurnContext::GithubReview(_))));
}
