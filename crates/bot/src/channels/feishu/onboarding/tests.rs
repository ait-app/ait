use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::{Form, Json, Router, routing::post};
use secrecy::ExposeSecret;
use serde_json::{Value, json};

use super::*;
use crate::ports::Credentials;
use crate::test_support::{Fixture, WORKSPACE, setup};

const VERIFICATION: &str = "https://open.feishu.cn/page/launcher?user_code=ABCD&from=feishu";

#[derive(Debug, Default)]
struct Memory {
    secrets: StdMutex<BTreeMap<ChannelId, String>>,
    fail_store: AtomicBool,
}

impl Credentials for Memory {
    fn resolve(&self, _: &str) -> Result<SecretString, ErrorCode> {
        Err(ErrorCode::BotIo)
    }
    fn store(&self, channel: ChannelId, secret: &SecretString) -> Result<(), ErrorCode> {
        if self.fail_store.load(Ordering::SeqCst) {
            return Err(ErrorCode::BotIo);
        }
        self.secrets
            .lock()
            .unwrap()
            .insert(channel, secret.expose_secret().to_owned());
        Ok(())
    }
    fn delete(&self, channel: ChannelId) -> Result<(), ErrorCode> {
        self.secrets.lock().unwrap().remove(&channel);
        Ok(())
    }
}

/// Feishu stand-in: `polls` is answered in order, repeating its last entry.
struct Feishu {
    base: String,
    requests: Arc<StdMutex<Vec<BTreeMap<String, String>>>>,
}

async fn feishu(polls: Vec<(u16, Value)>) -> Feishu {
    let requests: Arc<StdMutex<Vec<BTreeMap<String, String>>>> = Arc::default();
    let seen = requests.clone();
    let polls = Arc::new(StdMutex::new(polls));
    let app = Router::new()
        .route(
            REGISTRATION_PATH,
            post(move |Form(form): Form<BTreeMap<String, String>>| {
                let seen = seen.clone();
                let polls = polls.clone();
                async move {
                    let action = form["action"].clone();
                    seen.lock().unwrap().push(form);
                    let (status, body) = match action.as_str() {
                        "init" => (200, json!({"supported_auth_methods":["client_secret"]})),
                        "begin" => (
                            200,
                            json!({"device_code":"device-1","verification_uri_complete":VERIFICATION,"interval":1,"expires_in":3600}),
                        ),
                        _ => {
                            let mut polls = polls.lock().unwrap();
                            if polls.len() > 1 {
                                polls.remove(0)
                            } else {
                                polls[0].clone()
                            }
                        }
                    };
                    (
                        axum::http::StatusCode::from_u16(status).unwrap(),
                        Json(body),
                    )
                }
            }),
        )
        .route(
            TOKEN_PATH,
            post(|Json(body): Json<Value>| async move {
                let accepted = body["app_secret"] == "good-secret";
                Json(json!({"code":if accepted {0} else {10014}}))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Feishu { base, requests }
}

fn created() -> Value {
    json!({
        "client_id":"cli_new",
        "client_secret":"new-secret",
        "user_info":{"open_id":"ou_scanner","tenant_brand":"feishu"},
    })
}

fn pending() -> (u16, Value) {
    (400, json!({"error":"authorization_pending"}))
}

struct Harness {
    bots: Bots,
    onboarding: Onboarding,
    credentials: Arc<Memory>,
    _fixture: Fixture,
}

async fn harness(feishu: &Feishu, credentials: Memory) -> Harness {
    let fixture = Fixture::new();
    let credentials = Arc::new(credentials);
    let bots = fixture.bots_with(credentials.clone());
    bots.execute("bot.setup.request", setup(WORKSPACE))
        .await
        .unwrap();
    Harness {
        bots,
        onboarding: Onboarding::at(&feishu.base, &feishu.base),
        credentials,
        _fixture: fixture,
    }
}

impl Harness {
    async fn start(&self) -> Uuid {
        let started = self
            .onboarding
            .start(
                &self.bots,
                ScanStart {
                    workspace_id: WORKSPACE.into(),
                    allow_groups: true,
                },
            )
            .await
            .unwrap();
        started["scanId"].as_str().unwrap().parse().unwrap()
    }

    async fn status(&self, scan_id: Uuid) -> Value {
        self.onboarding
            .status(&self.bots, request(scan_id))
            .await
            .unwrap()
    }

    /// Make the next status call ask Feishu instead of waiting out the poll interval.
    async fn due(&self) {
        let scan = self.onboarding.scans.get(WORKSPACE).unwrap();
        scan.lock().await.next_poll = Instant::now();
    }

    async fn channels(&self) -> Vec<Value> {
        let listed = self
            .bots
            .execute("bot.channel.list.request", json!({"workspaceId":WORKSPACE}))
            .await
            .unwrap();
        listed["channels"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|channel| channel["config"]["kind"] == "feishu")
            .cloned()
            .collect()
    }
}

fn request(scan_id: Uuid) -> ScanRequest {
    ScanRequest {
        workspace_id: WORKSPACE.into(),
        scan_id,
    }
}

fn connect_request(channel_id: ChannelId, secret: &str) -> FeishuConnect {
    FeishuConnect {
        workspace_id: WORKSPACE.into(),
        channel_id,
        app_id: "cli_manual".into(),
        app_secret: secret.into(),
        allowed_senders: vec!["ou_alice".into()],
        allow_groups: false,
    }
}

#[tokio::test]
async fn approved_scan_stores_the_secret_and_enables_a_channel_for_the_scanner() {
    let feishu = feishu(vec![pending(), (200, created())]).await;
    let harness = harness(&feishu, Memory::default()).await;
    let scan_id = harness.start().await;

    assert_eq!(harness.status(scan_id).await, json!({"state":"waiting"}));
    harness.due().await;
    let complete = harness.status(scan_id).await;

    assert_eq!(complete["state"], "complete");
    let channel = &complete["channel"];
    let id: ChannelId = channel["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(channel["enabled"], true);
    assert_eq!(channel["config"]["appId"], "cli_new");
    assert_eq!(channel["config"]["allowedSenders"], json!(["ou_scanner"]));
    assert_eq!(channel["config"]["allowGroups"], true);
    assert_eq!(
        channel["config"]["secretRef"],
        stored_credential_reference(id)
    );
    assert!(!channel.to_string().contains("new-secret"));
    assert_eq!(
        harness
            .credentials
            .secrets
            .lock()
            .unwrap()
            .get(&id)
            .unwrap(),
        "new-secret"
    );
    assert_eq!(harness.status(scan_id).await, complete);
    assert_eq!(harness.channels().await.len(), 1);
}

#[tokio::test]
async fn a_status_before_the_interval_does_not_ask_feishu_again() {
    let feishu = feishu(vec![pending()]).await;
    let harness = harness(&feishu, Memory::default()).await;
    let scan_id = harness.start().await;

    harness.status(scan_id).await;
    harness.status(scan_id).await;

    let polls = feishu
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|form| form["action"] == "poll")
        .count();
    assert_eq!(polls, 1);
}

#[tokio::test]
async fn slow_down_lengthens_the_poll_interval() {
    let feishu = feishu(vec![(400, json!({"error":"slow_down"}))]).await;
    let harness = harness(&feishu, Memory::default()).await;
    let scan_id = harness.start().await;

    assert_eq!(harness.status(scan_id).await["state"], "waiting");

    let scan = harness.onboarding.scans.get(WORKSPACE).unwrap();
    assert_eq!(scan.lock().await.interval, Duration::from_secs(6));
}

#[tokio::test]
async fn terminal_feishu_answers_are_reported_and_create_nothing() {
    for (error, reason) in [("access_denied", "denied"), ("expired_token", "expired")] {
        let feishu = feishu(vec![(400, json!({"error":error}))]).await;
        let harness = harness(&feishu, Memory::default()).await;
        let scan_id = harness.start().await;

        let status = harness.status(scan_id).await;

        assert_eq!(status, json!({"state":"failed","reason":reason}));
        assert!(harness.channels().await.is_empty());
    }
}

#[tokio::test]
async fn lark_tenants_are_refused_because_the_channel_only_reaches_feishu() {
    let mut lark = created();
    lark["user_info"]["tenant_brand"] = json!("lark");
    let feishu = feishu(vec![(200, lark)]).await;
    let harness = harness(&feishu, Memory::default()).await;
    let scan_id = harness.start().await;

    assert_eq!(harness.status(scan_id).await["reason"], "larkUnsupported");
    assert!(harness.credentials.secrets.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_scan_without_the_scanner_identity_cannot_set_an_allowed_sender() {
    let mut anonymous = created();
    anonymous["user_info"] = json!({"tenant_brand":"feishu"});
    let feishu = feishu(vec![(200, anonymous)]).await;
    let harness = harness(&feishu, Memory::default()).await;
    let scan_id = harness.start().await;

    assert_eq!(harness.status(scan_id).await["reason"], "unavailable");
    assert!(harness.channels().await.is_empty());
}

#[tokio::test]
async fn an_unrecognized_feishu_answer_keeps_waiting() {
    let feishu = feishu(vec![(502, json!({"error":"server_error"}))]).await;
    let harness = harness(&feishu, Memory::default()).await;
    let scan_id = harness.start().await;

    assert_eq!(harness.status(scan_id).await["state"], "waiting");
}

#[tokio::test]
async fn an_approved_scan_survives_a_failed_save_and_retries_without_polling_feishu() {
    let feishu = feishu(vec![(200, created())]).await;
    let memory = Memory {
        fail_store: AtomicBool::new(true),
        ..Memory::default()
    };
    let harness = harness(&feishu, memory).await;
    let scan_id = harness.start().await;

    assert_eq!(harness.status(scan_id).await["state"], "waiting");
    assert!(harness.channels().await.is_empty());

    harness
        .credentials
        .fail_store
        .store(false, Ordering::SeqCst);
    harness.due().await;
    assert_eq!(harness.status(scan_id).await["state"], "complete");
    assert_eq!(harness.channels().await.len(), 1);
    let polls = feishu
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|form| form["action"] == "poll")
        .count();
    assert_eq!(polls, 1);
}

#[tokio::test]
async fn a_paused_bot_fails_an_approved_scan_instead_of_retrying() {
    let feishu = feishu(vec![pending(), (200, created())]).await;
    let harness = harness(&feishu, Memory::default()).await;
    let scan_id = harness.start().await;
    harness.status(scan_id).await;
    harness
        .bots
        .execute("bot.pause.request", json!({"workspaceId":WORKSPACE}))
        .await
        .unwrap();

    harness.due().await;

    assert_eq!(harness.status(scan_id).await["reason"], "unavailable");
    assert!(harness.credentials.secrets.lock().unwrap().is_empty());
}

#[tokio::test]
async fn expired_unknown_and_replaced_scans_report_expired() {
    let feishu = feishu(vec![pending()]).await;
    let harness = harness(&feishu, Memory::default()).await;
    assert_eq!(harness.status(Uuid::new_v4()).await["reason"], "expired");

    let first = harness.start().await;
    let second = harness.start().await;
    assert_eq!(harness.status(first).await["reason"], "expired");

    let scan = harness.onboarding.scans.get(WORKSPACE).unwrap();
    scan.lock().await.deadline = Instant::now();
    assert_eq!(harness.status(second).await["reason"], "expired");
}

#[tokio::test]
async fn cancel_forgets_only_the_named_scan() {
    let feishu = feishu(vec![pending()]).await;
    let harness = harness(&feishu, Memory::default()).await;
    let scan_id = harness.start().await;

    let other = harness.onboarding.cancel(request(Uuid::new_v4())).await;
    assert_eq!(other.unwrap(), json!({"cancelled":false}));
    let cancelled = harness.onboarding.cancel(request(scan_id)).await;
    assert_eq!(cancelled.unwrap(), json!({"cancelled":true}));
    assert_eq!(harness.status(scan_id).await["reason"], "expired");
}

#[tokio::test]
async fn cancel_stops_a_status_already_holding_the_scan_from_creating_a_channel() {
    let feishu = feishu(vec![(200, created())]).await;
    let harness = harness(&feishu, Memory::default()).await;
    let scan_id = harness.start().await;
    let held = harness.onboarding.scans.get(WORKSPACE).unwrap();

    assert_eq!(
        harness.onboarding.cancel(request(scan_id)).await.unwrap(),
        json!({"cancelled":true})
    );

    let scan = held.lock().await;
    assert_eq!(scan.outcome.as_ref().unwrap()["reason"], "expired");
    drop(scan);
    assert!(harness.channels().await.is_empty());
}

#[tokio::test]
async fn cancel_reports_false_after_the_channel_was_created() {
    let feishu = feishu(vec![(200, created())]).await;
    let harness = harness(&feishu, Memory::default()).await;
    let scan_id = harness.start().await;
    assert_eq!(harness.status(scan_id).await["state"], "complete");

    assert_eq!(
        harness.onboarding.cancel(request(scan_id)).await.unwrap(),
        json!({"cancelled":false})
    );
    assert_eq!(harness.channels().await.len(), 1);
}

#[tokio::test]
async fn a_scan_requires_an_active_bot() {
    let feishu = feishu(vec![pending()]).await;
    let harness = harness(&feishu, Memory::default()).await;
    harness
        .bots
        .execute("bot.pause.request", json!({"workspaceId":WORKSPACE}))
        .await
        .unwrap();

    let refused = harness
        .onboarding
        .start(
            &harness.bots,
            ScanStart {
                workspace_id: WORKSPACE.into(),
                allow_groups: false,
            },
        )
        .await;

    assert_eq!(refused.unwrap_err(), ErrorCode::BotConflict);
    assert!(feishu.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn manual_connect_verifies_stores_and_retries_without_rewriting() {
    let feishu = feishu(vec![pending()]).await;
    let harness = harness(&feishu, Memory::default()).await;
    let id = ChannelId::generate();

    let refused = harness
        .onboarding
        .connect(&harness.bots, connect_request(id, "wrong-secret"))
        .await;
    assert_eq!(refused.unwrap_err(), ErrorCode::InvalidMessage);
    assert!(harness.credentials.secrets.lock().unwrap().is_empty());

    let connected = harness
        .onboarding
        .connect(&harness.bots, connect_request(id, "good-secret"))
        .await
        .unwrap();
    assert_eq!(connected["created"], true);
    assert_eq!(connected["channel"]["enabled"], true);

    harness.credentials.secrets.lock().unwrap().clear();
    let retried = harness
        .onboarding
        .connect(&harness.bots, connect_request(id, "good-secret"))
        .await
        .unwrap();
    assert_eq!(retried["created"], false);
    assert!(harness.credentials.secrets.lock().unwrap().is_empty());
}

#[tokio::test]
async fn manual_connect_rejects_malformed_secrets_before_contacting_feishu() {
    let feishu = feishu(vec![pending()]).await;
    let harness = harness(&feishu, Memory::default()).await;

    for secret in ["", "line\nbreak"] {
        let refused = harness
            .onboarding
            .connect(
                &harness.bots,
                connect_request(ChannelId::generate(), secret),
            )
            .await;
        assert_eq!(refused.unwrap_err(), ErrorCode::InvalidMessage);
    }
}

#[tokio::test]
async fn deleting_a_channel_removes_its_stored_secret() {
    let feishu = feishu(vec![(200, created())]).await;
    let harness = harness(&feishu, Memory::default()).await;
    let scan_id = harness.start().await;
    let complete = harness.status(scan_id).await;
    let channel = complete["channel"]["id"].clone();

    harness
        .bots
        .execute(
            "bot.channel.delete.request",
            json!({"workspaceId":WORKSPACE,"channelId":channel}),
        )
        .await
        .unwrap();

    for _ in 0..100 {
        if harness.credentials.secrets.lock().unwrap().is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the deleted channel's secret was not removed");
}

#[test]
fn scan_link_keeps_feishus_code_and_adds_the_requested_permissions() {
    let link = url::Url::parse(&scan_link(VERIFICATION).unwrap()).unwrap();
    let pairs: BTreeMap<String, String> = link.query_pairs().into_owned().collect();

    assert_eq!(pairs["user_code"], "ABCD");
    assert_eq!(pairs["from"], "sdk");
    assert_eq!(pairs["createOnly"], "true");
    let compressed = URL_SAFE_NO_PAD.decode(&pairs["addons"]).unwrap();
    let mut json = String::new();
    std::io::Read::read_to_string(
        &mut flate2::read::GzDecoder::new(compressed.as_slice()),
        &mut json,
    )
    .unwrap();
    let addons: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(
        addons["events"]["items"]["tenant"],
        json!(["im.message.receive_v1"])
    );
    assert_eq!(addons["scopes"]["tenant"].as_array().unwrap().len(), 3);
}

#[test]
fn scan_link_refuses_anything_outside_feishu_over_https() {
    for link in [
        "http://open.feishu.cn/page",
        "https://open.feishu.cn.example.com/page",
        "https://example.com/page",
        "not a url",
    ] {
        assert_eq!(scan_link(link).unwrap_err(), ErrorCode::BotIo, "{link}");
    }
}
