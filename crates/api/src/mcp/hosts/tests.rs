use std::sync::{Arc, Mutex};
use std::time::Duration;

use model::peer::{Error, Host, HostState, Peers, Request};
use serde_json::{Value, json};
use tokio::sync::Notify;
use uuid::Uuid;

use super::{MIN_PEER_CALL_TIME, OUTCOME_UNKNOWN, PEER_CALL_TIMEOUT, route, tagged};
use crate::mcp::catalog;

/// One sibling Host behind a fake Hub link, which records the time limit of every call.
#[derive(Debug)]
struct Link {
    target: Host,
    /// How long listing the Hosts takes, on the paused clock; `None`: it never ends.
    listing: Option<Duration>,
    /// The reply to every call; `None`: no reply ever arrives.
    answer: Option<Value>,
    /// Signalled when the Hosts are listed.
    listed: Notify,
    /// Signalled when a call is handed over.
    sent: Notify,
    timeouts: Mutex<Vec<Duration>>,
}

#[async_trait::async_trait]
impl Peers for Link {
    async fn hosts(&self) -> Result<Vec<Host>, Error> {
        self.listed.notify_one();
        match self.listing {
            Some(listing) => tokio::time::sleep(listing).await, // virtual time
            None => std::future::pending().await,
        }
        Ok(vec![self.target.clone()])
    }

    async fn call(&self, _: Uuid, _: Request, timeout: Duration) -> Result<Value, Error> {
        self.timeouts.lock().unwrap().push(timeout);
        self.sent.notify_one();
        match &self.answer {
            Some(answer) => Ok(answer.clone()),
            None => std::future::pending().await,
        }
    }
}

fn link(listing: Option<Duration>, answer: Option<Value>) -> Arc<Link> {
    Arc::new(Link {
        target: Host {
            server_id: Uuid::new_v4(),
            host_id: Uuid::new_v4(),
            node_id: Uuid::new_v4(),
            name: "Build".to_owned(),
            hostname: "build.local".to_owned(),
            platform: "linux".to_owned(),
            app_version: "1.2.3".to_owned(),
            state: HostState::Online,
            supported: true,
            enabled: true,
        },
        listing,
        answer,
        listed: Notify::new(),
        sent: Notify::new(),
        timeouts: Mutex::new(Vec::new()),
    })
}

/// A tool result the other Host may answer with.
fn answer() -> Value {
    json!({"content":[{"type":"text","text":"{}"}],"structuredContent":{},"isError":false})
}

fn api(link: &Arc<Link>) -> crate::Api {
    crate::Api::new(
        "127.0.0.1:7316".parse().unwrap(),
        "stable".into(),
        "instance".into(),
        "offline-test-token-at-least-32-characters".into(),
        crate::Services::default(),
    )
    .unwrap()
    .with_peers(link.clone())
}

/// Route `agent_get` to the Host of `link` without a caller Agent.
async fn routed(api: crate::Api, link: Arc<Link>) -> Value {
    let (tool, _) = catalog::resolve("agent_get").unwrap();
    let host = link.target.server_id.to_string();
    route(api.shared.clone(), tool, json!({"agentId":"x"}), None, host)
        .await
        .unwrap()
}

fn timeouts(link: &Link) -> Vec<Duration> {
    link.timeouts.lock().unwrap().clone()
}

/// Assert that `result` is the failure `code` of a call to the Host of `link`.
fn failed(result: &Value, link: &Link, code: &str) {
    assert_eq!(result["isError"], true, "{result}");
    assert_eq!(
        result["structuredContent"]["hostId"],
        link.target.server_id.to_string(),
        "{result}"
    );
    assert_eq!(result["structuredContent"]["code"], code, "{result}");
}

#[tokio::test(start_paused = true)]
async fn the_call_gets_what_remains_of_one_deadline() {
    let listing = Duration::from_secs(20);
    let link = link(Some(listing), Some(answer()));

    let result = routed(api(&link), link.clone()).await;

    assert_eq!(result["isError"], false, "{result}");
    assert_eq!(
        timeouts(&link),
        [PEER_CALL_TIMEOUT.checked_sub(listing).unwrap()]
    );
}

#[tokio::test(start_paused = true)]
async fn with_too_little_time_left_no_call_is_made() {
    let listing =
        PEER_CALL_TIMEOUT.checked_sub(MIN_PEER_CALL_TIME).unwrap() + Duration::from_millis(1);
    let link = link(Some(listing), Some(answer()));

    let result = routed(api(&link), link.clone()).await;

    failed(&result, &link, "hub_unavailable");
    assert!(timeouts(&link).is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_sent_call_given_up_at_the_call_limit_has_an_unknown_outcome() {
    let link = link(Some(Duration::ZERO), None);

    let result = routed(api(&link), link.clone()).await;

    failed(&result, &link, "outcome_unknown");
    assert_eq!(result["content"][0]["text"], OUTCOME_UNKNOWN);
    assert_eq!(timeouts(&link).len(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_sent_call_given_up_by_a_drain_has_an_unknown_outcome() {
    let link = link(Some(Duration::ZERO), None);
    let api = api(&link);
    let call = tokio::spawn(routed(api.clone(), link.clone()));
    link.sent.notified().await;

    api.begin_shutdown();

    let result = call.await.unwrap();
    failed(&result, &link, "outcome_unknown");
    assert_eq!(timeouts(&link).len(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_call_drained_before_it_was_sent_keeps_the_drain_error() {
    let link = link(None, Some(answer()));
    let api = api(&link);
    let call = tokio::spawn(routed(api.clone(), link.clone()));
    link.listed.notified().await;

    api.begin_shutdown();

    let result = call.await.unwrap();
    assert_eq!(
        result,
        json!({"content":[{"type":"text","text":model::ErrorCode::ServerDraining.message()}],
            "isError":true}),
    );
    assert!(timeouts(&link).is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_reply_is_tagged_unless_it_is_not_a_tool_result() {
    let bare = json!({"content":[{"type":"text","text":"done"}],"isError":false});
    let link = link(Some(Duration::ZERO), Some(bare.clone()));

    let result = routed(api(&link), link.clone()).await;

    let mut expected = bare;
    expected["structuredContent"] = json!({"hostId":link.target.server_id.to_string()});
    assert_eq!(result, expected);

    // A reply the link should have refused as malformed: the call ran, its answer is unreadable.
    for unreadable in [json!("text"), json!({"structuredContent":[1]})] {
        assert_eq!(tagged(unreadable.clone(), "h"), None, "{unreadable}");
        let link = self::link(Some(Duration::ZERO), Some(unreadable));
        let result = routed(api(&link), link.clone()).await;
        failed(&result, &link, "outcome_unknown");
    }
}
