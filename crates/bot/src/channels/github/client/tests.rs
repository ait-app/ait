use domain::bot::review_status::ReviewStatusPage;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use domain::bot::{
    DeliveryStatus, GithubReviewContext, GithubReviewTrigger, Turn, TurnContext, TurnStatus,
};
use domain::{BotTurnId, ConversationId};
use secrecy::SecretString;
use serde_json::{Value, json};

use super::{Board, Failure, GithubClient, Published};
use crate::state::StatusWritten;
use domain::bot::StatusLanguage;
use domain::bot::review_status::{ReviewStatus, ReviewStatusKey, StatusEvent};

const INSTALLATION: &str = "9";

/// A scripted answer: its status, extra headers and body.
type Reply = (u16, Vec<(&'static str, String)>, Option<Value>);

/// A GitHub API that answers each request with the next scripted reply (200 when none is
/// left) and records what it was asked.
#[derive(Clone, Default)]
struct FakeGithub {
    replies: Arc<Mutex<VecDeque<Reply>>>,
    seen: Arc<Mutex<Vec<(String, Value)>>>,
}

impl FakeGithub {
    fn reply(&self, status: u16, headers: &[(&'static str, String)]) -> &Self {
        self.replies
            .lock()
            .unwrap()
            .push_back((status, headers.to_vec(), None));
        self
    }

    fn reply_with(&self, status: u16, body: Value) -> &Self {
        self.replies
            .lock()
            .unwrap()
            .push_back((status, Vec::new(), Some(body)));
        self
    }

    fn requests(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|(path, _)| path.clone())
            .collect()
    }

    fn paths(&self) -> Vec<String> {
        self.requests()
            .into_iter()
            .map(|request| request.split_once(' ').unwrap().1.to_owned())
            .collect()
    }

    fn body(&self, index: usize) -> Value {
        self.seen.lock().unwrap()[index].1.clone()
    }

    async fn start(&self) -> GithubClient {
        let app = Router::new().fallback(answer).with_state(self.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = GithubClient::at(reqwest::Client::new(), &format!("http://{address}"));
        // A fresh token, so no App JWT has to be signed.
        *client.lock() = Some((
            INSTALLATION.to_owned(),
            SecretString::from("cached-token"),
            std::time::Instant::now(),
        ));
        client
    }
}

async fn answer(
    State(github): State<FakeGithub>,
    method: axum::http::Method,
    uri: Uri,
    body: Bytes,
) -> Response {
    let body = serde_json::from_slice(&body).unwrap_or(Value::Null);
    github
        .seen
        .lock()
        .unwrap()
        .push((format!("{method} {}", uri.path()), body));
    let (status, headers, reply) =
        github
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or((200, Vec::new(), None));
    let mut map = HeaderMap::new();
    for (name, value) in headers {
        map.insert(
            HeaderName::from_static(name),
            HeaderValue::from_str(&value).unwrap(),
        );
    }
    let reply = reply.unwrap_or_else(
        || json!({"id":1,"html_url":"https://github.com/owner/repository/pull/7#review-1"}),
    );
    (
        StatusCode::from_u16(status).unwrap(),
        map,
        axum::Json(reply),
    )
        .into_response()
}

fn review_turn(status: TurnStatus) -> Turn {
    let report = json!({"summary":"Looks risky","findings":[{"path":"src/lib.rs","line":3,
        "side":"RIGHT","severity":"high","title":"Unchecked index","body":"May panic."}]});
    Turn {
        id: BotTurnId::generate(),
        conversation_id: ConversationId::generate(),
        message_id: "github-review:1".to_owned(),
        sequence: 1,
        generation: 0,
        text: "review".to_owned(),
        bot_sequence: None,
        sender_type: None,
        passive: false,
        reply_to: None,
        context: Some(TurnContext::GithubReview(Box::new(GithubReviewContext {
            repository: "owner/repository".to_owned(),
            project_id: "project-1".to_owned(),
            pull_number: 7,
            title: "Change".to_owned(),
            url: "https://github.com/owner/repository/pull/7".to_owned(),
            base_ref: "main".to_owned(),
            head_ref: "feature".to_owned(),
            base_sha: "1".repeat(40),
            head_sha: "2".repeat(40),
            trigger: GithubReviewTrigger::Mention,
            trigger_user: "maintainer".to_owned(),
            trigger_comment_id: None,
            workspace_id: "wks_review".to_owned(),
        }))),
        reply: Some(report.to_string()),
        messages: Vec::new(),
        pending_permissions: Vec::new(),
        status,
        delivery: DeliveryStatus::Pending,
        error: (status == TurnStatus::Failed).then(|| "checkout failed".to_owned()),
        created_at: "2026-11-28T00:00:00Z".to_owned(),
        wake: None,
        silent: false,
        notification_cursor: None,
    }
}

async fn submit(client: &GithubClient, turn: &Turn) -> Result<Option<Published>, Failure> {
    client
        .submit("1", INSTALLATION, &SecretString::from("not a key"), turn)
        .await
}

async fn no_wait(_: std::time::Duration) {}

const REVIEWS: &str = "/repos/owner/repository/pulls/7/reviews";
const COMMENTS: &str = "/repos/owner/repository/issues/7/comments";

#[tokio::test]
async fn a_rate_limited_review_is_posted_once_after_github_lets_it_through() {
    let github = FakeGithub::default();
    github
        .reply(429, &[("retry-after", "0".to_owned())])
        .reply(403, &[("retry-after", "0".to_owned())]);
    let client = github.start().await;
    let turn = review_turn(TurnStatus::Completed);
    let published = GithubClient::retrying(|| submit(&client, &turn), no_wait)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        published,
        Published {
            high: 1,
            medium: 0,
            low: 0,
            url: Some("https://github.com/owner/repository/pull/7#review-1".to_owned()),
        }
    );
    // The cached token served every attempt; only the third review was accepted.
    assert_eq!(github.paths(), [REVIEWS, REVIEWS, REVIEWS]);
    assert_eq!(github.body(2)["comments"][0]["path"], "src/lib.rs");
    assert_eq!(github.body(2)["commit_id"], "2".repeat(40));
}

#[tokio::test]
async fn an_invalid_review_moves_its_findings_into_the_body_then_into_a_comment() {
    let github = FakeGithub::default();
    github.reply(422, &[]).reply(422, &[]);
    let client = github.start().await;
    submit(&client, &review_turn(TurnStatus::Completed))
        .await
        .unwrap();
    assert_eq!(github.paths(), [REVIEWS, REVIEWS, COMMENTS]);
    assert_eq!(github.body(1)["comments"], json!([]));
    let fallback = github.body(1)["body"].as_str().unwrap().to_owned();
    assert!(fallback.contains("Unchecked index"), "{fallback}");
    assert_eq!(github.body(2)["body"], fallback);
}

#[tokio::test]
async fn a_review_that_did_not_complete_posts_nothing_of_its_own() {
    let github = FakeGithub::default();
    let client = github.start().await;
    for status in [
        TurnStatus::Failed,
        TurnStatus::Uncertain,
        TurnStatus::Cancelled,
    ] {
        assert_eq!(submit(&client, &review_turn(status)).await, Ok(None));
    }
    // The outcome is shown on the status comment instead.
    assert!(github.paths().is_empty());
}

#[tokio::test]
async fn errors_whose_effect_is_unknown_are_not_repeated() {
    let github = FakeGithub::default();
    github.reply(502, &[]).reply(403, &[]);
    let client = github.start().await;
    let turn = review_turn(TurnStatus::Completed);
    assert_eq!(
        GithubClient::retrying(|| submit(&client, &turn), no_wait).await,
        Err(Failure::Final)
    );
    // A plain 403 is a missing permission, not a rate limit.
    assert_eq!(submit(&client, &turn).await, Err(Failure::Final));
    assert_eq!(github.paths(), [REVIEWS, REVIEWS]);
}

#[tokio::test]
async fn an_exhausted_rate_limit_waits_until_its_reset() {
    let github = FakeGithub::default();
    let reset = chrono::Utc::now().timestamp() + 30;
    github.reply(
        403,
        &[
            ("x-ratelimit-remaining", "0".to_owned()),
            ("x-ratelimit-reset", reset.to_string()),
        ],
    );
    let client = github.start().await;
    let Err(Failure::Later(wait)) = submit(&client, &review_turn(TurnStatus::Completed)).await
    else {
        panic!("an exhausted rate limit is retried later");
    };
    assert!(wait.as_secs() >= 25 && wait.as_secs() <= 30, "{wait:?}");
}

#[tokio::test]
async fn a_rejected_token_is_dropped_so_the_next_attempt_mints_another() {
    let github = FakeGithub::default();
    github.reply(401, &[]);
    let client = github.start().await;
    assert_eq!(
        submit(&client, &review_turn(TurnStatus::Completed)).await,
        Err(Failure::Later(std::time::Duration::ZERO))
    );
    assert!(client.cached(INSTALLATION).is_none());
    // Minting needs the App key; a key that is not one fails for good, without a request.
    assert_eq!(
        submit(&client, &review_turn(TurnStatus::Completed)).await,
        Err(Failure::Final)
    );
    assert_eq!(github.paths(), [REVIEWS]);
}

#[tokio::test]
async fn a_token_is_reused_only_for_its_installation() {
    let github = FakeGithub::default();
    let client = github.start().await;
    assert!(client.cached(INSTALLATION).is_some());
    assert!(client.cached("10").is_none());
    client
        .pull(
            "1",
            INSTALLATION,
            &SecretString::from("not a key"),
            "owner/repository",
            7,
        )
        .await
        .unwrap();
    assert_eq!(github.paths(), ["/repos/owner/repository/pulls/7"]);
}

fn status_board(comment: Option<u64>) -> ReviewStatus {
    let mut status = ReviewStatus::new(ReviewStatusKey {
        channel_id: "d4119547-46ff-4a0c-82e1-5b921ca2d25e".parse().unwrap(),
        repository: "owner/repository".to_owned(),
        pull_number: 7,
    });
    status.append(
        &"2".repeat(40),
        StatusEvent::Preparing,
        "2026-11-28T08:45:00Z",
    );
    if let Some(comment_id) = comment {
        status.pages.push(ReviewStatusPage {
            comment_id,
            body_hash: String::new(),
        });
    }
    status
}

async fn sync(client: &GithubClient, status: &ReviewStatus) -> Result<StatusWritten, Failure> {
    let board = Board {
        status,
        mention: "ait-bot",
        language: StatusLanguage::En,
    };
    client
        .sync_status("1", INSTALLATION, &SecretString::from("not a key"), &board)
        .await
}

const ISSUE_COMMENTS: &str = "/repos/owner/repository/issues/7/comments";

#[tokio::test]
async fn the_first_status_reacts_then_creates_its_comment_with_the_marker() {
    let github = FakeGithub::default();
    github
        .reply(201, &[])
        .reply_with(200, json!([]))
        .reply_with(201, json!({"id": 77}));
    let client = github.start().await;
    let mut status = status_board(None);
    status.react(10);
    let written = sync(&client, &status).await.unwrap();
    assert_eq!(
        github.requests(),
        [
            "POST /repos/owner/repository/issues/comments/10/reactions",
            &format!("GET {ISSUE_COMMENTS}"),
            &format!("POST {ISSUE_COMMENTS}"),
        ]
    );
    assert_eq!(github.body(0)["content"], "eyes");
    let body = github.body(2)["body"].as_str().unwrap().to_owned();
    assert!(
        body.starts_with("<!-- ait-review-status:v1 channel=d4119547"),
        "{body}"
    );
    assert_eq!(
        (
            written.pages[0].comment_id,
            written.revision,
            written.reacted
        ),
        (77, 1, vec![10])
    );
}

#[tokio::test]
async fn an_existing_status_comment_is_found_by_its_marker_and_edited() {
    let github = FakeGithub::default();
    let marker =
        "<!-- ait-review-status:v1 channel=d4119547-46ff-4a0c-82e1-5b921ca2d25e page=0 -->\nold";
    github.reply_with(
        200,
        json!([
            {"id": 5, "user": {"login": "someone"}, "body": marker},
            {"id": 6, "user": {"login": "ait-bot[bot]"}, "body": marker},
        ]),
    );
    let client = github.start().await;
    let written = sync(&client, &status_board(None)).await.unwrap();
    assert_eq!(
        github.requests(),
        [
            format!("GET {ISSUE_COMMENTS}"),
            "PATCH /repos/owner/repository/issues/comments/6".to_owned(),
        ]
    );
    assert_eq!(written.pages[0].comment_id, 6);
}

#[tokio::test]
async fn a_deleted_status_comment_is_created_again() {
    let github = FakeGithub::default();
    github
        .reply(404, &[])
        .reply_with(200, json!([]))
        .reply_with(201, json!({"id": 8}));
    let client = github.start().await;
    let written = sync(&client, &status_board(Some(6))).await.unwrap();
    assert_eq!(
        github.requests(),
        [
            "PATCH /repos/owner/repository/issues/comments/6".to_owned(),
            format!("GET {ISSUE_COMMENTS}"),
            format!("POST {ISSUE_COMMENTS}"),
        ]
    );
    assert_eq!(written.pages[0].comment_id, 8);
}

#[tokio::test]
async fn a_board_github_already_shows_only_sends_its_reactions() {
    let github = FakeGithub::default();
    let client = github.start().await;
    let mut status = status_board(Some(6));
    status.published_revision = status.revision;
    assert_eq!(sync(&client, &status).await.unwrap().pages[0].comment_id, 6);
    assert!(github.requests().is_empty());
}

#[tokio::test]
async fn an_overflow_comment_preserves_old_lines_and_links_both_ways() {
    let github = FakeGithub::default();
    github
        .reply(200, &[])
        .reply_with(200, json!([]))
        .reply_with(201, json!({"id": 8}))
        .reply(200, &[]);
    let client = github.start().await;
    let mut status = status_board(Some(6));
    for index in 0..110 {
        status.append(
            &"2".repeat(40),
            StatusEvent::Failed {
                reason: format!("line-{index:03} {}", "x".repeat(501)),
            },
            "2026-11-28T08:46:00Z",
        );
    }
    let written = sync(&client, &status).await.unwrap();
    assert_eq!(
        written
            .pages
            .iter()
            .map(|page| page.comment_id)
            .collect::<Vec<_>>(),
        [6, 8]
    );
    let old = github.body(3)["body"].as_str().unwrap().to_owned();
    let new = github.body(2)["body"].as_str().unwrap().to_owned();
    assert!(old.contains("line-000") && !new.contains("line-000"));
    assert!(new.contains("line-109") && !old.contains("line-109"));
    assert!(old.contains("#issuecomment-8") && new.contains("#issuecomment-6"));
    assert_eq!(
        old.matches("line-").count() + new.matches("line-").count(),
        110
    );
}
