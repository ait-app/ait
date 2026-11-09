use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use axum::http::{HeaderMap, StatusCode};
use axum::{Json, Router, routing::post};
use domain::ChannelId;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey};
use uuid::Uuid;

use super::*;
use crate::test_support::{Fixture, MemorySecrets, WORKSPACE, setup};

const NONCE: [u8; NONCE_LEN] = [7; NONCE_LEN];

fn seal(secret: &str, key: &str) -> String {
    let key = STANDARD.decode(key).unwrap();
    let key = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &key).unwrap());
    let mut sealed = secret.as_bytes().to_vec();
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(NONCE),
        Aad::empty(),
        &mut sealed,
    )
    .unwrap();
    let mut framed = NONCE.to_vec();
    framed.extend(sealed);
    STANDARD.encode(framed)
}

/// QQ portal stand-in: `polls` builds each poll response from the key the daemon supplied.
async fn portal(polls: Vec<fn(&str) -> Value>) -> String {
    let key = Arc::new(StdMutex::new(String::new()));
    let polls = Arc::new(StdMutex::new(polls));
    let stored = key.clone();
    let app = Router::new()
        .route(
            CREATE_PATH,
            post(move |headers: HeaderMap, Json(body): Json<Value>| {
                let stored = stored.clone();
                async move {
                    assert_eq!(headers["accept"], "application/json");
                    *stored.lock().unwrap() = body["key"].as_str().unwrap().to_owned();
                    Json(json!({"retcode":0,"data":{"task_id":"task 1"}}))
                }
            }),
        )
        .route(
            POLL_PATH,
            post(move |Json(body): Json<Value>| {
                let key = key.clone();
                let polls = polls.clone();
                async move {
                    assert_eq!(body["task_id"], "task 1");
                    let key = key.lock().unwrap().clone();
                    let mut polls = polls.lock().unwrap();
                    let next = if polls.len() > 1 {
                        polls.remove(0)
                    } else {
                        polls[0]
                    };
                    Json(next(&key))
                }
            }),
        )
        .route(
            "/token",
            post(|Json(body): Json<Value>| async move {
                if body["clientSecret"] == "good-secret" {
                    (
                        StatusCode::OK,
                        Json(json!({"access_token":"token","expires_in":"7200"})),
                    )
                } else {
                    (
                        StatusCode::UNAUTHORIZED,
                        Json(json!({"code":100_016,"message":"invalid"})),
                    )
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

fn pending(_: &str) -> Value {
    json!({"retcode":0,"data":{"status":1}})
}

fn completed_scan(key: &str) -> Value {
    json!({"retcode":0,"data":{"status":2,"bot_appid":"10001","user_openid":"scanner-open",
        "bot_encrypt_secret":seal("new-secret",key)}})
}

fn expired(_: &str) -> Value {
    json!({"retcode":0,"data":{"status":3}})
}

fn portal_error(_: &str) -> Value {
    json!({"retcode":500,"msg":"busy"})
}

fn undecryptable(_: &str) -> Value {
    json!({"retcode":0,"data":{"status":2,"bot_appid":"10001","user_openid":"scanner-open",
        "bot_encrypt_secret":"AAAA"}})
}

struct Harness {
    bots: Bots,
    onboarding: Onboarding,
    credentials: Arc<MemorySecrets>,
    _fixture: Fixture,
}

async fn harness(base: &str) -> Harness {
    let fixture = Fixture::new();
    let credentials = Arc::new(MemorySecrets::default());
    let bots = fixture.bots_with(credentials.clone());
    bots.execute("bot.setup.request", setup(WORKSPACE))
        .await
        .unwrap();
    Harness {
        bots,
        onboarding: Onboarding::at(base, &format!("{base}/token")),
        credentials,
        _fixture: fixture,
    }
}

impl Harness {
    async fn start(&self, allow_groups: bool) -> Uuid {
        let started = self
            .onboarding
            .start(
                &self.bots,
                ScanStart {
                    workspace_id: WORKSPACE.into(),
                    allow_groups,
                },
            )
            .await
            .unwrap();
        assert_eq!(
            started["url"],
            "https://q.qq.com/qqbot/openclaw/connect.html?task_id=task+1&_wv=2&source=ait"
        );
        started["scanId"].as_str().unwrap().parse().unwrap()
    }

    async fn status(&self, scan_id: Uuid) -> Value {
        let scan = self.onboarding.scans.get(WORKSPACE).unwrap();
        scan.lock().await.next_poll = Instant::now();
        self.onboarding
            .status(
                &self.bots,
                ScanRequest {
                    workspace_id: WORKSPACE.into(),
                    scan_id,
                },
            )
            .await
            .unwrap()
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
            .filter(|channel| channel["config"]["kind"] == "qq")
            .cloned()
            .collect()
    }
}

#[tokio::test]
async fn a_completed_scan_decrypts_the_secret_and_allows_only_the_scanner() {
    let base = portal(vec![pending, portal_error, completed_scan]).await;
    let harness = harness(&base).await;
    let scan_id = harness.start(true).await;
    assert_eq!(harness.status(scan_id).await["state"], "waiting");
    assert_eq!(harness.status(scan_id).await["state"], "waiting");
    assert_eq!(harness.status(scan_id).await["state"], "complete");
    let channels = harness.channels().await;
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0]["enabled"], true);
    let config = &channels[0]["config"];
    assert_eq!(config["appId"], "10001");
    assert_eq!(config["allowedSenders"], json!(["scanner-open"]));
    assert_eq!(config["allowGroups"], true);
    assert_eq!(harness.credentials.stored(), vec!["new-secret".to_owned()]);
    harness.bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_expired_task_fails_and_a_bad_secret_creates_nothing() {
    for (poll, reason) in [
        (expired as fn(&str) -> Value, FAILED_EXPIRED),
        (undecryptable as fn(&str) -> Value, FAILED_UNAVAILABLE),
    ] {
        let base = portal(vec![poll]).await;
        let harness = harness(&base).await;
        let scan_id = harness.start(false).await;
        assert_eq!(harness.status(scan_id).await, failed(reason));
        assert!(harness.channels().await.is_empty());
        harness.bots.shutdown().await.unwrap();
    }
}

#[test]
fn a_secret_sealed_with_another_key_is_refused() {
    let key = random_key().unwrap();
    let other = random_key().unwrap();
    let sealed = seal("secret", expose(&other));
    assert_eq!(decrypt_secret(&sealed, &key).err(), Some(ErrorCode::BotIo));
    let opened = decrypt_secret(&seal("secret", expose(&key)), &key).unwrap();
    assert_eq!(expose(&opened), "secret");
}

fn connect_request(channel_id: ChannelId, secret: &str) -> QqConnect {
    QqConnect {
        workspace_id: WORKSPACE.into(),
        channel_id,
        app_id: "10001".into(),
        app_secret: secret.into(),
        allow_groups: false,
    }
}

#[tokio::test]
async fn pasted_credentials_are_verified_then_stored_awaiting_pairing() {
    let base = portal(vec![pending]).await;
    let harness = harness(&base).await;
    let refused = harness
        .onboarding
        .connect(
            &harness.bots,
            connect_request(ChannelId::generate(), "wrong"),
        )
        .await;
    assert_eq!(refused.err(), Some(ErrorCode::InvalidMessage));
    assert!(harness.credentials.stored().is_empty());
    let created = harness
        .onboarding
        .connect(
            &harness.bots,
            connect_request(ChannelId::generate(), "good-secret"),
        )
        .await
        .unwrap();
    assert_eq!(created["channel"]["enabled"], true);
    assert_eq!(created["channel"]["config"]["allowedSenders"], json!([]));
    assert_eq!(harness.credentials.stored(), vec!["good-secret".to_owned()]);
    harness.bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn pasted_secrets_with_control_characters_are_refused_before_any_request() {
    let harness = harness("http://127.0.0.1:1").await;
    for secret in ["", "bad\nsecret"] {
        let refused = harness
            .onboarding
            .connect(
                &harness.bots,
                connect_request(ChannelId::generate(), secret),
            )
            .await;
        assert_eq!(refused.err(), Some(ErrorCode::InvalidMessage));
    }
    harness.bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn pairing_codes_work_once_and_only_for_their_channel() {
    let base = portal(vec![pending]).await;
    let harness = harness(&base).await;
    let created = harness
        .onboarding
        .connect(
            &harness.bots,
            connect_request(ChannelId::generate(), "good-secret"),
        )
        .await
        .unwrap();
    let channel_id: ChannelId = created["channel"]["id"].as_str().unwrap().parse().unwrap();
    let issued = harness
        .onboarding
        .pair(
            &harness.bots,
            QqPairRequest {
                workspace_id: WORKSPACE.into(),
                channel_id,
            },
        )
        .await
        .unwrap();
    assert_eq!(issued["expiresInSeconds"], PAIR_CODE_SECONDS);
    let code = issued["code"].as_str().unwrap();
    assert_eq!(code.len(), PAIR_CODE_LENGTH);
    assert!(code.bytes().all(|byte| PAIR_CODE_ALPHABET.contains(&byte)));
    assert!(!harness.onboarding.claim(ChannelId::generate(), code));
    assert!(!harness.onboarding.claim(channel_id, "WRONGCOD"));
    assert!(
        harness
            .onboarding
            .claim(channel_id, &format!(" {} ", code.to_lowercase()))
    );
    assert!(!harness.onboarding.claim(channel_id, code));
    harness.bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_expired_pairing_code_is_refused() {
    let onboarding = Onboarding::at("http://127.0.0.1:1", "http://127.0.0.1:1/token");
    let channel = ChannelId::generate();
    onboarding.codes().insert(
        channel,
        PairCode {
            code: "ABCDEFGH".into(),
            expires: Instant::now(),
        },
    );
    assert!(!onboarding.claim(channel, "ABCDEFGH"));
    assert!(onboarding.codes().is_empty());
}

#[tokio::test]
async fn pairing_is_refused_for_a_channel_the_workspace_does_not_own() {
    let base = portal(vec![pending]).await;
    let harness = harness(&base).await;
    let refused = harness
        .onboarding
        .pair(
            &harness.bots,
            QqPairRequest {
                workspace_id: WORKSPACE.into(),
                channel_id: ChannelId::generate(),
            },
        )
        .await;
    assert!(refused.is_err());
    assert!(harness.onboarding.codes().is_empty());
    harness.bots.shutdown().await.unwrap();
}
