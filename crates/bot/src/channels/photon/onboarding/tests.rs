use std::sync::{Arc, Mutex as StdMutex};

use axum::extract::Query;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use domain::ChannelId;
use tokio::net::TcpListener;

use super::*;
use crate::service::Bots;
use crate::test_support::{Fixture, MemorySecrets, WORKSPACE, setup};

const PROJECT: &str = "pho_prj_0123456789abcdefghjkmnpqrs";
const KEY: &str = "pho_sk_offline";
const HANDLE: &str = "+15551234567";
const LINE: &str = "+15550000000";

type AssignRequest = (Option<String>, Option<String>, Value);

#[derive(Default)]
struct Mock {
    assign: StdMutex<Option<(StatusCode, Value)>>,
    assignments: StdMutex<Vec<Value>>,
    messages: StdMutex<Vec<Value>>,
    messages_status: StdMutex<Option<StatusCode>>,
    message_queries: StdMutex<Vec<Vec<(String, String)>>>,
    assign_requests: StdMutex<Vec<AssignRequest>>,
}

async fn platform() -> (String, Arc<Mock>) {
    let mock = Arc::new(Mock::default());
    let (assigning, listing, reading) = (mock.clone(), mock.clone(), mock.clone());
    let assignments = format!("/v1/projects/{PROJECT}/platforms/imessage/assignments");
    let app = Router::new()
        .route(
            &assignments,
            post(move |headers: HeaderMap, Json(body): Json<Value>| {
                let mock = assigning.clone();
                async move {
                    let header = |name: &str| {
                        headers
                            .get(name)
                            .map(|value| value.to_str().unwrap().to_owned())
                    };
                    mock.assign_requests.lock().unwrap().push((
                        header("authorization"),
                        header("idempotency-key"),
                        body,
                    ));
                    let answer = mock.assign.lock().unwrap().clone().unwrap_or((
                        StatusCode::CREATED,
                        json!({"assignedPhoneNumber":LINE,"handle":HANDLE}),
                    ));
                    (answer.0, Json(answer.1))
                }
            }),
        )
        .route(
            &assignments,
            get(move || {
                let mock = listing.clone();
                async move {
                    let assignments = mock.assignments.lock().unwrap().clone();
                    Json(json!({"assignments":assignments}))
                }
            }),
        )
        .route(
            &format!("/v1/projects/{PROJECT}/messages"),
            get(move |Query(query): Query<Vec<(String, String)>>| {
                let mock = reading.clone();
                async move {
                    mock.message_queries.lock().unwrap().push(query);
                    let status = mock
                        .messages_status
                        .lock()
                        .unwrap()
                        .unwrap_or(StatusCode::OK);
                    let items = mock.messages.lock().unwrap().clone();
                    (status, Json(json!({"items":items})))
                }
            }),
        );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, mock)
}

struct Harness {
    bots: Bots,
    onboarding: Onboarding,
    credentials: Arc<MemorySecrets>,
    mock: Arc<Mock>,
    _fixture: Fixture,
}

async fn harness() -> Harness {
    let (base, mock) = platform().await;
    let fixture = Fixture::new();
    let credentials = Arc::new(MemorySecrets::default());
    let bots = fixture.bots_with(credentials.clone());
    bots.execute("bot.setup.request", setup(WORKSPACE))
        .await
        .unwrap();
    Harness {
        bots,
        onboarding: Onboarding::at(&base),
        credentials,
        mock,
        _fixture: fixture,
    }
}

fn start_request(project: &str, key: &str, handle: &str) -> ImessagePairStart {
    ImessagePairStart {
        workspace_id: WORKSPACE.to_owned(),
        project_id: project.to_owned(),
        api_key: key.to_owned(),
        handle: handle.to_owned(),
    }
}

fn message(handle: &str, text: &str) -> Value {
    json!({"status":"received","content":{"type":"text","text":text},
        "sentBy":{"handle":handle}})
}

impl Harness {
    async fn start(&self) -> Value {
        self.onboarding
            .start(&self.bots, start_request(PROJECT, KEY, HANDLE))
            .await
            .unwrap()
    }

    fn pairing_request(started: &Value) -> PairRequest {
        PairRequest {
            workspace_id: WORKSPACE.to_owned(),
            pairing_id: started["pairingId"].as_str().unwrap().parse().unwrap(),
        }
    }

    /// Ask for status with the poll interval already elapsed.
    async fn status(&self, started: &Value) -> Value {
        let session = self.onboarding.pairings.get(WORKSPACE).unwrap();
        session.lock().await.next_poll = Instant::now();
        self.onboarding
            .status(&self.bots, Self::pairing_request(started))
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn the_code_from_the_right_handle_creates_a_channel_for_it() {
    let harness = harness().await;
    let started = harness.start().await;
    let code = started["code"].as_str().unwrap().to_owned();
    assert_eq!(started["state"], "waiting");
    assert_eq!(started["phoneNumber"], LINE);
    assert_eq!(started["url"], format!("sms:{LINE}&body={code}"));
    assert!(is_pairing_code(&code));
    {
        let requests = harness.mock.assign_requests.lock().unwrap();
        assert_eq!(requests[0].0.as_deref(), Some("Bearer pho_sk_offline"));
        assert!(requests[0].1.as_deref().unwrap().starts_with("ait-assign:"));
        assert_eq!(requests[0].2, json!({"handle":HANDLE}));
    }

    assert_eq!(harness.status(&started).await["state"], "waiting");
    harness
        .mock
        .messages
        .lock()
        .unwrap()
        .push(message(HANDLE, &format!(" {code} ")));
    let complete = harness.status(&started).await;

    assert_eq!(complete["state"], "complete");
    let config: ChannelConfig =
        serde_json::from_value(complete["channel"]["config"].clone()).unwrap();
    let ChannelConfig::PhotonImessage {
        project_id,
        key_ref,
        allowed_senders,
    } = config
    else {
        panic!("expected an iMessage channel");
    };
    assert_eq!(project_id, PROJECT);
    assert!(key_ref.starts_with("env:AIT_SERVER_CREDENTIAL_STORED_"));
    assert_eq!(allowed_senders, [HANDLE]);
    assert_eq!(harness.credentials.stored(), [KEY]);

    let query = harness.mock.message_queries.lock().unwrap()[0].clone();
    for (name, value) in [
        ("direction", "received"),
        ("platform", "imessage"),
        ("order", "desc"),
    ] {
        assert!(
            query.contains(&(name.to_owned(), value.to_owned())),
            "{name}"
        );
    }
    assert!(query.iter().any(|(name, _)| name == "createdSince"));
    // A later status returns the same outcome without another read.
    let reads = harness.mock.message_queries.lock().unwrap().len();
    assert_eq!(harness.status(&started).await, complete);
    assert_eq!(harness.mock.message_queries.lock().unwrap().len(), reads);
}

#[tokio::test]
async fn a_wrong_code_or_sender_does_not_pair() {
    let harness = harness().await;
    let started = harness.start().await;
    let code = started["code"].as_str().unwrap().to_owned();
    {
        let mut messages = harness.mock.messages.lock().unwrap();
        messages.push(message("+15559999999", &code));
        messages.push(message(HANDLE, "ait-00000000000000000000000000000000"));
        messages.push(
            json!({"status":"accepted","content":{"type":"text","text":code},
            "sentBy":{"handle":HANDLE}}),
        );
    }

    assert_eq!(harness.status(&started).await["state"], "waiting");
    assert!(harness.credentials.stored().is_empty());
}

#[tokio::test]
async fn invalid_input_is_refused_before_any_request() {
    let harness = harness().await;
    for (request, reason) in [
        (start_request("pho_prj_bad", KEY, HANDLE), "invalidKey"),
        (start_request(PROJECT, "sk-other", HANDLE), "invalidKey"),
        (start_request(PROJECT, "pho_sk_a b", HANDLE), "invalidKey"),
        (start_request(PROJECT, KEY, "not-a-handle"), "invalidHandle"),
        (start_request(PROJECT, KEY, "+0123456"), "invalidHandle"),
    ] {
        let outcome = harness
            .onboarding
            .start(&harness.bots, request)
            .await
            .unwrap();
        assert_eq!(outcome, json!({"state":"failed","reason":reason}));
    }
    assert!(harness.mock.assign_requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_existing_assignment_is_reused() {
    let harness = harness().await;
    *harness.mock.assign.lock().unwrap() =
        Some((StatusCode::CONFLICT, json!({"code":"ASSIGNMENT_EXISTS"})));
    {
        let mut assignments = harness.mock.assignments.lock().unwrap();
        assignments.push(json!({"handle":"+15557777777","assignedPhoneNumber":"+15551110000"}));
        assignments.push(json!({"handle":HANDLE,"assignedPhoneNumber":LINE}));
    }

    let started = harness.start().await;

    assert_eq!(started["state"], "waiting");
    assert_eq!(started["phoneNumber"], LINE);
}

#[tokio::test]
async fn assignment_refusals_map_to_failure_reasons() {
    let cases = [
        (
            StatusCode::CONFLICT,
            json!({"code":"IMESSAGE_MODE_DEDICATED"}),
            "dedicatedLine",
        ),
        (StatusCode::CONFLICT, json!({"code":"OTHER"}), "unavailable"),
        (StatusCode::UNAUTHORIZED, json!({}), "invalidKey"),
        (StatusCode::FORBIDDEN, json!({}), "invalidKey"),
        (StatusCode::UNPROCESSABLE_ENTITY, json!({}), "invalidHandle"),
        (StatusCode::BAD_GATEWAY, json!({}), "unavailable"),
    ];
    for (status, body, reason) in cases {
        let harness = harness().await;
        *harness.mock.assign.lock().unwrap() = Some((status, body));
        assert_eq!(
            harness.start().await,
            json!({"state":"failed","reason":reason}),
            "{status}"
        );
    }
}

#[tokio::test]
async fn a_revoked_key_fails_the_pairing_but_an_outage_keeps_waiting() {
    let harness = harness().await;
    let started = harness.start().await;
    *harness.mock.messages_status.lock().unwrap() = Some(StatusCode::BAD_GATEWAY);
    assert_eq!(harness.status(&started).await["state"], "waiting");

    *harness.mock.messages_status.lock().unwrap() = Some(StatusCode::UNAUTHORIZED);
    let failed = json!({"state":"failed","reason":"invalidKey"});
    assert_eq!(harness.status(&started).await, failed);
    assert_eq!(harness.status(&started).await, failed);
}

#[tokio::test]
async fn an_expired_pairing_fails_and_a_cancelled_one_is_forgotten() {
    let harness = harness().await;
    let started = harness.start().await;
    let session = harness.onboarding.pairings.get(WORKSPACE).unwrap();
    session.lock().await.deadline = Instant::now();
    assert_eq!(
        harness.status(&started).await,
        json!({"state":"failed","reason":"expired"})
    );

    let started = harness.start().await;
    let cancelled = harness
        .onboarding
        .cancel(Harness::pairing_request(&started))
        .await
        .unwrap();
    assert_eq!(cancelled, json!({"cancelled":true}));
    let status = harness
        .onboarding
        .status(&harness.bots, Harness::pairing_request(&started))
        .await
        .unwrap();
    assert_eq!(status, json!({"state":"failed","reason":"expired"}));
}

#[test]
fn handles_are_checked_for_shape() {
    for handle in ["+15551234567", "+12", "person@example.com"] {
        assert!(valid_handle(handle), "{handle}");
    }
    for handle in [
        "",
        "+",
        "+0123",
        "+1234567890123456",
        "person@",
        "@example.com",
        "a b@c.d",
    ] {
        assert!(!valid_handle(handle), "{handle}");
    }
}

#[test]
fn only_generated_codes_are_pairing_codes() {
    assert!(is_pairing_code(&format!(
        "{CODE_PREFIX}{}",
        Uuid::new_v4().simple()
    )));
    for text in [
        "hello",
        "ait-",
        "ait-xyz",
        "ait-0123456789abcdef0123456789abcdeg",
    ] {
        assert!(!is_pairing_code(text), "{text}");
    }
}

#[tokio::test]
async fn a_project_with_a_channel_is_refused_even_while_that_channel_is_disabled() {
    let harness = harness().await;
    harness
        .bots
        .execute(
            "bot.channel.create.request",
            json!({
                "workspaceId":WORKSPACE,"channelId":ChannelId::generate(),"enabled":false,
                "config":{"kind":"photon_imessage","projectId":PROJECT,
                    "keyRef":"env:AIT_SERVER_CREDENTIAL_TEST","allowedSenders":[HANDLE]},
            }),
        )
        .await
        .unwrap();

    assert_eq!(
        harness.start().await,
        json!({"state":"failed","reason":"alreadyConnected"})
    );
    assert!(harness.mock.assign_requests.lock().unwrap().is_empty());
}
