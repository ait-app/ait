use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use axum::extract::Query;
use axum::{Json, Router, routing::get};
use tokio::time::Instant;
use uuid::Uuid;

use super::*;
use crate::test_support::{Fixture, MemorySecrets, WORKSPACE, setup};

const LINK: &str = "https://liteapp.weixin.qq.com/q/abc?qrcode=qr-1&bot_type=3";

/// iLink stand-in: `polls` is answered in order, repeating its last entry.
async fn ilink(link: &'static str, polls: Vec<Value>) -> (String, Arc<StdMutex<Vec<String>>>) {
    let queried = Arc::new(StdMutex::new(Vec::new()));
    let seen = queried.clone();
    let polls = Arc::new(StdMutex::new(polls));
    let app = Router::new()
        .route(
            "/ilink/bot/get_bot_qrcode",
            get(
                move |Query(query): Query<BTreeMap<String, String>>| async move {
                    assert_eq!(query["bot_type"], "3");
                    Json(json!({"qrcode":"qr-1","qrcode_img_content":link}))
                },
            ),
        )
        .route(
            "/ilink/bot/get_qrcode_status",
            get(move |Query(query): Query<BTreeMap<String, String>>| {
                let seen = seen.clone();
                let polls = polls.clone();
                async move {
                    seen.lock().unwrap().push(query["qrcode"].clone());
                    let mut polls = polls.lock().unwrap();
                    let next = if polls.len() > 1 {
                        polls.remove(0)
                    } else {
                        polls[0].clone()
                    };
                    Json(next)
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, queried)
}

fn confirmed_status() -> Value {
    json!({
        "status":"confirmed",
        "ilink_bot_id":"bot-1@im.bot",
        "bot_token":"new-token",
        "baseurl":"https://ilinkai.weixin.qq.com/",
        "ilink_user_id":"scanner@im.wechat",
    })
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
        onboarding: Onboarding::at(base),
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
        assert_eq!(started["url"], LINK);
        started["scanId"].as_str().unwrap().parse().unwrap()
    }

    async fn status(&self, scan_id: Uuid) -> Value {
        let scan = self.onboarding.scans.get(WORKSPACE).unwrap();
        scan.lock().await.next_poll = Instant::now();
        self.onboarding
            .status(&self.bots, request(scan_id))
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
            .filter(|channel| channel["config"]["kind"] == "weixin")
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

#[tokio::test]
async fn a_confirmed_scan_stores_the_token_and_allows_only_the_scanner() {
    let (base, queried) = ilink(LINK, vec![json!({"status":"wait"}), confirmed_status()]).await;
    let harness = harness(&base).await;
    let scan_id = harness.start().await;
    assert_eq!(harness.status(scan_id).await["state"], "waiting");
    let outcome = harness.status(scan_id).await;
    assert_eq!(outcome["state"], "complete");
    assert_eq!(queried.lock().unwrap()[0], "qr-1");
    let channels = harness.channels().await;
    assert_eq!(channels.len(), 1);
    let config = &channels[0]["config"];
    assert_eq!(channels[0]["enabled"], true);
    assert_eq!(config["accountId"], "bot-1@im.bot");
    assert_eq!(config["baseUrl"], "https://ilinkai.weixin.qq.com");
    assert_eq!(config["allowedSenders"], json!(["scanner@im.wechat"]));
    assert!(config.get("allowGroups").is_none());
    assert_eq!(harness.credentials.stored(), vec!["new-token".to_owned()]);
    // A finished scan answers from memory.
    assert_eq!(harness.status(scan_id).await["state"], "complete");
    assert_eq!(harness.channels().await.len(), 1);
    harness.bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_confirmation_naming_a_foreign_host_creates_nothing() {
    let mut status = confirmed_status();
    status["baseurl"] = json!("https://collector.example");
    let (base, _) = ilink(LINK, vec![status]).await;
    let harness = harness(&base).await;
    let scan_id = harness.start().await;
    let outcome = harness.status(scan_id).await;
    assert_eq!(outcome, failed(FAILED_UNAVAILABLE));
    assert!(harness.channels().await.is_empty());
    assert!(harness.credentials.stored().is_empty());
    harness.bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_confirmation_without_the_scanner_creates_nothing() {
    let mut status = confirmed_status();
    status.as_object_mut().unwrap().remove("ilink_user_id");
    let (base, _) = ilink(LINK, vec![status]).await;
    let harness = harness(&base).await;
    let scan_id = harness.start().await;
    assert_eq!(harness.status(scan_id).await, failed(FAILED_UNAVAILABLE));
    assert!(harness.channels().await.is_empty());
    harness.bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_expired_code_fails_and_stays_failed() {
    let (base, _) = ilink(LINK, vec![json!({"status":"expired"})]).await;
    let harness = harness(&base).await;
    let scan_id = harness.start().await;
    assert_eq!(harness.status(scan_id).await, failed(FAILED_EXPIRED));
    assert_eq!(harness.status(scan_id).await, failed(FAILED_EXPIRED));
    harness.bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn redirects_are_followed_only_within_wechat() {
    let scan_flow = |status: Value| classify(&status);
    assert!(matches!(
        scan_flow(json!({"status":"scaned_but_redirect","redirect_host":"sz.weixin.qq.com"})),
        Poll::Redirect(base) if base == "https://sz.weixin.qq.com"
    ));
    assert!(matches!(
        scan_flow(json!({"status":"scaned_but_redirect","redirect_host":"collector.example"})),
        Poll::Waiting
    ));
    assert!(matches!(
        scan_flow(
            json!({"status":"scaned_but_redirect","redirect_host":"evil.com/.weixin.qq.com"})
        ),
        Poll::Waiting
    ));
    assert!(matches!(
        scan_flow(json!({"status":"scaned"})),
        Poll::Waiting
    ));
}

#[tokio::test]
async fn a_link_outside_wechat_is_refused() {
    let (base, _) = ilink(
        "https://collector.example/q",
        vec![json!({"status":"wait"})],
    )
    .await;
    let harness = harness(&base).await;
    let started = harness
        .onboarding
        .start(
            &harness.bots,
            ScanStart {
                workspace_id: WORKSPACE.into(),
                allow_groups: false,
            },
        )
        .await;
    assert_eq!(started, Err(ErrorCode::BotIo));
    harness.bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_cancelled_scan_never_creates_a_channel() {
    let (base, _) = ilink(LINK, vec![confirmed_status()]).await;
    let harness = harness(&base).await;
    let scan_id = harness.start().await;
    let cancelled = harness.onboarding.cancel(request(scan_id)).await.unwrap();
    assert_eq!(cancelled, json!({"cancelled":true}));
    let after = harness
        .onboarding
        .status(&harness.bots, request(scan_id))
        .await
        .unwrap();
    assert_eq!(after, failed(FAILED_EXPIRED));
    assert!(harness.channels().await.is_empty());
    harness.bots.shutdown().await.unwrap();
}
