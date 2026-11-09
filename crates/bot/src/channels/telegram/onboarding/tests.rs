use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use axum::extract::Path;
use axum::{Json, Router, routing::post};
use secrecy::ExposeSecret;
use serde_json::{Value, json};

use super::*;
use crate::ports::Credentials;
use crate::test_support::{Fixture, WORKSPACE, setup};

const TOKEN: &str = "12345:abcdefghijklmnopqrstuvwxyz0123";

#[derive(Debug, Default)]
struct Memory {
    secrets: Mutex<BTreeMap<ChannelId, String>>,
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
}

#[derive(Clone, Default)]
struct Script {
    me: Arc<Mutex<Option<Value>>>,
    webhook: Arc<Mutex<Option<Value>>>,
    updates: Arc<Mutex<VecDeque<Value>>>,
    update_requests: Arc<Mutex<Vec<Value>>>,
}

fn accepted(result: &Value) -> Value {
    json!({"ok":true,"result":result})
}

fn refused(code: u16) -> Value {
    json!({"ok":false,"error_code":code,"description":"refused"})
}

async fn telegram(script: Script) -> String {
    let app = Router::new().route(
        "/{*path}",
        post(move |Path(path): Path<String>, Json(body): Json<Value>| {
            let script = script.clone();
            async move {
                assert!(path.starts_with(&format!("bot{TOKEN}/")), "{path}");
                Json(match path.rsplit('/').next().unwrap() {
                    "getMe" => script.me.lock().unwrap().clone().unwrap_or_else(|| {
                        accepted(&json!({"id":12345,"username":"ait_test_bot"}))
                    }),
                    "getWebhookInfo" => script
                        .webhook
                        .lock()
                        .unwrap()
                        .clone()
                        .unwrap_or_else(|| accepted(&json!({"url":""}))),
                    _ => {
                        script.update_requests.lock().unwrap().push(body);
                        script
                            .updates
                            .lock()
                            .unwrap()
                            .pop_front()
                            .unwrap_or_else(|| accepted(&json!([])))
                    }
                })
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

struct Harness {
    bots: Bots,
    onboarding: Onboarding,
    credentials: Arc<Memory>,
    script: Script,
    _fixture: Fixture,
}

async fn harness(script: Script) -> Harness {
    let base = telegram(script.clone()).await;
    let fixture = Fixture::new();
    let credentials = Arc::new(Memory::default());
    let bots = fixture.bots_with(credentials.clone());
    bots.execute("bot.setup.request", setup(WORKSPACE))
        .await
        .unwrap();
    Harness {
        bots,
        onboarding: Onboarding::at(&base),
        credentials,
        script,
        _fixture: fixture,
    }
}

fn start_request(token: &str) -> TelegramPairStart {
    TelegramPairStart {
        workspace_id: WORKSPACE.into(),
        token: token.into(),
        allow_groups: true,
    }
}

fn request(pairing_id: Uuid) -> PairRequest {
    PairRequest {
        workspace_id: WORKSPACE.into(),
        pairing_id,
    }
}

fn message(update_id: i64, chat_type: &str, is_bot: bool, sender: i64, text: &str) -> Value {
    json!({"update_id":update_id,"message":{
        "chat":{"id":sender,"type":chat_type},
        "from":{"id":sender,"is_bot":is_bot},
        "text":text,
    }})
}

impl Harness {
    async fn start(&self) -> (Uuid, String) {
        let started = self
            .onboarding
            .start(&self.bots, start_request(TOKEN))
            .await
            .unwrap();
        assert_eq!(started["state"], "waiting");
        let url = started["url"].as_str().unwrap();
        let code = url.rsplit("start=").next().unwrap().to_owned();
        (
            started["pairingId"].as_str().unwrap().parse().unwrap(),
            code,
        )
    }

    async fn status(&self, id: Uuid) -> Value {
        self.onboarding
            .status(&self.bots, request(id))
            .await
            .unwrap()
    }

    async fn due(&self) {
        let pairing = self.onboarding.pairings.get(WORKSPACE).unwrap();
        pairing.lock().await.next_poll = Instant::now();
    }

    fn script_updates(&self, updates: Vec<Value>) {
        self.script
            .updates
            .lock()
            .unwrap()
            .push_back(accepted(&Value::Array(updates)));
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
            .filter(|channel| channel["config"]["kind"] == "telegram")
            .cloned()
            .collect()
    }
}

#[tokio::test]
async fn tapping_start_stores_the_token_and_enables_a_channel_for_that_person() {
    let harness = harness(Script::default()).await;
    let (id, code) = harness.start().await;
    harness.script_updates(vec![
        message(40, "private", false, 999, "/start"),
        message(41, "private", false, 777, &format!("/start {code}")),
    ]);

    let complete = harness.status(id).await;

    assert_eq!(complete["state"], "complete");
    let channel = &complete["channel"];
    let channel_id: ChannelId = channel["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(channel["enabled"], true);
    assert_eq!(channel["config"]["allowedSenders"], json!(["777"]));
    assert_eq!(channel["config"]["allowGroups"], true);
    assert_eq!(
        channel["config"]["tokenRef"],
        stored_credential_reference(channel_id)
    );
    assert!(!complete.to_string().contains(TOKEN));
    assert_eq!(
        harness.credentials.secrets.lock().unwrap()[&channel_id],
        TOKEN
    );
    {
        let requests = harness.script.update_requests.lock().unwrap();
        assert_eq!(requests[0]["offset"], 0);
        assert_eq!(
            requests[1]["offset"], 42,
            "the code message is acknowledged"
        );
    }
    assert_eq!(harness.status(id).await, complete);
}

#[tokio::test]
async fn the_link_opens_the_verified_bot_with_a_fresh_code() {
    let harness = harness(Script::default()).await;
    let started = harness
        .onboarding
        .start(&harness.bots, start_request(&format!("  {TOKEN}\n")))
        .await
        .unwrap();

    assert_eq!(started["botUsername"], "ait_test_bot");
    let url = started["url"].as_str().unwrap();
    let code = url
        .strip_prefix("https://t.me/ait_test_bot?start=")
        .unwrap();
    assert_eq!(code.len(), 32);
    assert!(!started.to_string().contains(TOKEN));
}

#[tokio::test]
async fn messages_that_are_not_the_code_from_a_person_in_private_are_ignored() {
    let harness = harness(Script::default()).await;
    let (id, code) = harness.start().await;
    let start = format!("/start {code}");
    harness.script_updates(vec![
        message(1, "group", false, 5, &start),
        message(2, "private", true, 6, &start),
        message(3, "private", false, 7, "/start wrong-code"),
        message(4, "private", false, 8, "hello"),
    ]);

    assert_eq!(harness.status(id).await["state"], "waiting");
    harness.due().await;
    harness.status(id).await;

    assert!(harness.channels().await.is_empty());
    let requests = harness.script.update_requests.lock().unwrap();
    assert_eq!(
        requests[1]["offset"], 5,
        "ignored updates are not read twice"
    );
}

#[tokio::test]
async fn a_status_before_the_interval_does_not_ask_telegram_again() {
    let harness = harness(Script::default()).await;
    let (id, _) = harness.start().await;

    harness.status(id).await;
    harness.status(id).await;

    assert_eq!(harness.script.update_requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn invalid_tokens_are_refused_with_a_specific_reason() {
    let script = Script::default();
    *script.me.lock().unwrap() = Some(refused(401));
    let harness = harness(script).await;

    for token in [
        TOKEN,
        "not-a-token",
        "abc:abcdefghijklmnopqrstuvwxyz0123",
        "1:short",
    ] {
        let result = harness
            .onboarding
            .start(&harness.bots, start_request(token))
            .await
            .unwrap();
        assert_eq!(result, json!({"state":"failed","reason":"invalidToken"}));
    }
}

#[tokio::test]
async fn an_existing_webhook_is_reported_and_never_removed() {
    let script = Script::default();
    *script.webhook.lock().unwrap() = Some(accepted(&json!({"url":"https://example.com/hook"})));
    let harness = harness(script).await;

    let result = harness
        .onboarding
        .start(&harness.bots, start_request(TOKEN))
        .await
        .unwrap();

    assert_eq!(result["reason"], "webhookConflict");
    assert!(harness.script.update_requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_temporary_webhook_lookup_failure_is_not_reported_as_a_conflict() {
    let script = Script::default();
    *script.webhook.lock().unwrap() = Some(refused(500));
    let harness = harness(script).await;

    let result = harness
        .onboarding
        .start(&harness.bots, start_request(TOKEN))
        .await
        .unwrap();

    assert_eq!(result["reason"], "unavailable");
}

#[tokio::test]
async fn a_bot_already_connected_to_a_channel_is_refused_without_reading_its_updates() {
    let harness = harness(Script::default()).await;
    let channel = ChannelId::generate();
    harness
        .bots
        .execute(
            "bot.channel.create.request",
            json!({"workspaceId":WORKSPACE,"channelId":channel,"enabled":true,"config":{
                "kind":"telegram","tokenRef":"env:AIT_SERVER_CREDENTIAL_TEST",
                "allowedSenders":["1"]}}),
        )
        .await
        .unwrap();
    harness
        .bots
        .account(channel, "telegram:12345".into())
        .await
        .unwrap();

    let result = harness
        .onboarding
        .start(&harness.bots, start_request(TOKEN))
        .await
        .unwrap();

    assert_eq!(
        result,
        json!({"state":"failed","reason":"alreadyConnected"})
    );
    assert!(harness.script.update_requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn another_program_reading_updates_ends_the_pairing() {
    let harness = harness(Script::default()).await;
    let (id, _) = harness.start().await;
    harness
        .script
        .updates
        .lock()
        .unwrap()
        .push_back(refused(409));

    assert_eq!(harness.status(id).await["reason"], "pollingConflict");
}

#[tokio::test]
async fn a_token_revoked_during_pairing_ends_it() {
    let harness = harness(Script::default()).await;
    let (id, _) = harness.start().await;
    harness
        .script
        .updates
        .lock()
        .unwrap()
        .push_back(refused(401));

    assert_eq!(harness.status(id).await["reason"], "invalidToken");
}

#[tokio::test]
async fn a_failed_save_retries_without_losing_the_sender() {
    let harness = harness(Script::default()).await;
    let (id, code) = harness.start().await;
    harness.credentials.fail_store.store(true, Ordering::SeqCst);
    harness.script_updates(vec![message(
        9,
        "private",
        false,
        77,
        &format!("/start {code}"),
    )]);

    assert_eq!(harness.status(id).await["state"], "waiting");

    harness
        .credentials
        .fail_store
        .store(false, Ordering::SeqCst);
    harness.due().await;
    let complete = harness.status(id).await;
    assert_eq!(
        complete["channel"]["config"]["allowedSenders"],
        json!(["77"])
    );
}

#[tokio::test]
async fn a_failed_acknowledgement_retries_before_creating_the_channel() {
    let harness = harness(Script::default()).await;
    let (id, code) = harness.start().await;
    harness.script_updates(vec![message(
        9,
        "private",
        false,
        77,
        &format!("/start {code}"),
    )]);
    harness
        .script
        .updates
        .lock()
        .unwrap()
        .push_back(refused(500));

    assert_eq!(harness.status(id).await["state"], "waiting");
    assert!(harness.channels().await.is_empty());

    harness.due().await;
    assert_eq!(harness.status(id).await["state"], "complete");
}

#[tokio::test]
async fn expired_unknown_and_replaced_pairings_report_expired() {
    let harness = harness(Script::default()).await;
    assert_eq!(harness.status(Uuid::new_v4()).await["reason"], "expired");

    let (first, _) = harness.start().await;
    let (second, _) = harness.start().await;
    assert_eq!(harness.status(first).await["reason"], "expired");

    let pairing = harness.onboarding.pairings.get(WORKSPACE).unwrap();
    pairing.lock().await.deadline = Instant::now();
    assert_eq!(harness.status(second).await["reason"], "expired");
}

#[tokio::test]
async fn cancel_applies_only_to_the_named_unfinished_pairing() {
    let harness = harness(Script::default()).await;
    let (id, _) = harness.start().await;

    let other = harness.onboarding.cancel(request(Uuid::new_v4())).await;
    assert_eq!(other.unwrap(), json!({"cancelled":false}));
    let cancelled = harness.onboarding.cancel(request(id)).await;
    assert_eq!(cancelled.unwrap(), json!({"cancelled":true}));
    assert_eq!(harness.status(id).await["reason"], "expired");
}

#[tokio::test]
async fn cancel_reports_false_after_the_channel_was_created() {
    let harness = harness(Script::default()).await;
    let (id, code) = harness.start().await;
    harness.script_updates(vec![message(
        9,
        "private",
        false,
        77,
        &format!("/start {code}"),
    )]);
    assert_eq!(harness.status(id).await["state"], "complete");

    let cancelled = harness.onboarding.cancel(request(id)).await;

    assert_eq!(cancelled.unwrap(), json!({"cancelled":false}));
    assert_eq!(harness.channels().await.len(), 1);
}

#[tokio::test]
async fn pairing_requires_an_active_bot() {
    let harness = harness(Script::default()).await;
    harness
        .bots
        .execute("bot.pause.request", json!({"workspaceId":WORKSPACE}))
        .await
        .unwrap();

    let refused = harness
        .onboarding
        .start(&harness.bots, start_request(TOKEN))
        .await;

    assert_eq!(refused.unwrap_err(), ErrorCode::BotConflict);
}

#[test]
fn only_a_start_message_carrying_a_code_counts_as_pairing() {
    let code = Uuid::new_v4().simple().to_string();
    assert!(is_pairing_start(&format!("/start {code}")));
    for text in [
        "/start".to_owned(),
        "/start hello".to_owned(),
        format!("please /start {code}"),
        format!("/start {code} extra"),
    ] {
        assert!(!is_pairing_start(&text), "{text}");
    }
}

#[test]
fn token_shape_matches_botfather_tokens() {
    assert!(valid_token(TOKEN));
    assert!(valid_token("1:abcdefghijklmnopqrstuvwxyz-_0123"));
    for token in [
        "",
        ":abcdefghijklmnopqrstuvwxyz0123",
        "12345:abc",
        "12345:abcdefghijklmnopqrstuvwxyz01/3",
    ] {
        assert!(!valid_token(token), "{token}");
    }
}
