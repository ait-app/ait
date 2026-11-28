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

use super::{Failure, GithubClient};

const INSTALLATION: &str = "9";

/// A scripted answer: its status and extra headers.
type Reply = (u16, Vec<(&'static str, String)>);

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
            .push_back((status, headers.to_vec()));
        self
    }

    fn paths(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|(path, _)| path.clone())
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

async fn answer(State(github): State<FakeGithub>, uri: Uri, body: Bytes) -> Response {
    let body = serde_json::from_slice(&body).unwrap_or(Value::Null);
    github
        .seen
        .lock()
        .unwrap()
        .push((uri.path().to_owned(), body));
    let (status, headers) = github
        .replies
        .lock()
        .unwrap()
        .pop_front()
        .unwrap_or((200, Vec::new()));
    let mut map = HeaderMap::new();
    for (name, value) in headers {
        map.insert(
            HeaderName::from_static(name),
            HeaderValue::from_str(&value).unwrap(),
        );
    }
    (
        StatusCode::from_u16(status).unwrap(),
        map,
        axum::Json(json!({"id":1})),
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

async fn submit(client: &GithubClient, turn: &Turn) -> Result<(), Failure> {
    client
        .submit("1", INSTALLATION, &SecretString::from("not a key"), turn)
        .await
}

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
    GithubClient::retrying(|| submit(&client, &turn))
        .await
        .unwrap();
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
async fn a_failed_review_leaves_a_comment_on_the_pull_request() {
    let github = FakeGithub::default();
    let client = github.start().await;
    submit(&client, &review_turn(TurnStatus::Failed))
        .await
        .unwrap();
    assert_eq!(github.paths(), [COMMENTS]);
    let body = github.body(0)["body"].as_str().unwrap().to_owned();
    assert!(body.contains("checkout failed"), "{body}");
}

#[tokio::test]
async fn an_unconfirmed_review_says_so_and_asks_for_a_new_commit() {
    let github = FakeGithub::default();
    let client = github.start().await;
    submit(&client, &review_turn(TurnStatus::Uncertain))
        .await
        .unwrap();
    assert_eq!(github.paths(), [COMMENTS]);
    let body = github.body(0)["body"].as_str().unwrap().to_owned();
    assert!(
        body.contains("could not be confirmed") && body.contains("new commit"),
        "{body}"
    );
}

#[tokio::test]
async fn errors_whose_effect_is_unknown_are_not_repeated() {
    let github = FakeGithub::default();
    github.reply(502, &[]).reply(403, &[]);
    let client = github.start().await;
    let turn = review_turn(TurnStatus::Completed);
    assert_eq!(
        GithubClient::retrying(|| submit(&client, &turn)).await,
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
