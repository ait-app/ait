use std::collections::VecDeque;
use std::sync::Mutex;

use model::hub::LinkState;
use tokio::sync::watch;

use super::*;
use crate::test_support::{Fixture, WORKSPACE, setup};

#[derive(Debug)]
struct FakeCenter {
    state: watch::Sender<LinkState>,
    calls: Mutex<Vec<(&'static str, Value)>>,
    results: Mutex<VecDeque<Result<Value, CallError>>>,
}

impl FakeCenter {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: watch::channel(LinkState::default()).0,
            calls: Mutex::default(),
            results: Mutex::default(),
        })
    }

    fn online(&self, online: bool, methods: &[&str]) {
        self.state.send_replace(LinkState {
            scope: Some(Scope {
                center: "https://center.test".to_owned(),
                node_session_id: uuid::Uuid::nil(),
            }),
            online,
            link: Some(uuid::Uuid::nil()),
            methods: methods.iter().map(|m| (*m).to_owned()).collect(),
        });
    }

    fn answer(&self, result: Result<Value, CallError>) {
        self.results.lock().unwrap().push_back(result);
    }

    fn calls(&self) -> Vec<(&'static str, Value)> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl Center for FakeCenter {
    fn state(&self) -> watch::Receiver<LinkState> {
        self.state.subscribe()
    }

    async fn call(
        &self,
        _scope: Scope,
        method: &'static str,
        params: Value,
    ) -> Result<Value, CallError> {
        self.calls.lock().unwrap().push((method, params));
        self.results
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Ok(json!({"deleted":true})))
    }
}

fn issued(secret: &str) -> Value {
    json!({
        "endpointId":"0193f2c4-7d1e-7a40-9c1b-5e2f8a6d3b11",
        "url":"https://aitapp.cn/api/v1/automation/ingress/webhook/0193f2c4-7d1e-7a40-9c1b-5e2f8a6d3b11",
        "secret":secret,
    })
}

async fn bots_with_center(fixture: &Fixture) -> (Bots, Arc<FakeCenter>) {
    let bots = fixture.bots();
    bots.execute("bot.setup.request", setup(WORKSPACE))
        .await
        .unwrap();
    let center = FakeCenter::new();
    bots.attach_center(center.clone());
    (bots, center)
}

async fn external_channels(bots: &Bots) -> Vec<Value> {
    let listed = bots
        .execute("bot.channel.list.request", json!({"workspaceId":WORKSPACE}))
        .await
        .unwrap();
    listed["channels"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|channel| channel["config"]["kind"] != "builtin" && channel["deleted"] != true)
        .cloned()
        .collect()
}

#[tokio::test]
async fn the_center_issues_an_endpoint_whose_secret_is_shown_once() {
    let fixture = Fixture::new();
    let (bots, center) = bots_with_center(&fixture).await;
    center.online(true, &[CREATE_METHOD, DELETE_METHOD]);
    center.answer(Ok(issued("s3cret-1")));
    let created = bots
        .execute(
            "bot.channel.webhook.create.request",
            json!({"workspaceId":WORKSPACE}),
        )
        .await
        .unwrap();
    assert_eq!(created["state"], "created");
    assert_eq!(created["secret"], "s3cret-1");
    let channel = &created["channel"];
    assert_eq!(channel["enabled"], true);
    assert_eq!(channel["config"]["kind"], "webhook");
    let calls = center.calls();
    assert_eq!(calls[0].0, CREATE_METHOD);
    assert_eq!(calls[0].1["workspaceId"], WORKSPACE);
    assert_eq!(calls[0].1["channelId"], channel["id"]);

    let listed = external_channels(&bots).await;
    assert_eq!(listed.len(), 1);
    assert!(
        !listed[0].to_string().contains("s3cret"),
        "the daemon never keeps the secret"
    );
    let published = bots.hub().borrow().clone();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].kind, "webhook");

    center.answer(Ok(issued("s3cret-2")));
    let rotated = bots
        .execute(
            "bot.channel.webhook.rotate.request",
            json!({"workspaceId":WORKSPACE,"channelId":channel["id"]}),
        )
        .await
        .unwrap();
    assert_eq!(rotated["state"], "rotated");
    assert_eq!(rotated["secret"], "s3cret-2");
    assert_eq!(center.calls()[1].1["channelId"], channel["id"]);

    bots.execute(
        "bot.channel.delete.request",
        json!({"workspaceId":WORKSPACE,"channelId":channel["id"]}),
    )
    .await
    .unwrap();
    for _ in 0..100 {
        if center.calls().len() == 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let calls = center.calls();
    assert_eq!(calls.len(), 3, "deleting the channel releases its endpoint");
    assert_eq!(calls[2].0, DELETE_METHOD);
    assert_eq!(calls[2].1, json!({"channelId":channel["id"]}));
}

#[tokio::test]
async fn center_failures_create_nothing() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    bots.execute("bot.setup.request", setup(WORKSPACE))
        .await
        .unwrap();
    let create = || {
        bots.execute(
            "bot.channel.webhook.create.request",
            json!({"workspaceId":WORKSPACE}),
        )
    };
    assert_eq!(create().await.unwrap()["reason"], FAILED_OFFLINE);

    let center = FakeCenter::new();
    bots.attach_center(center.clone());
    assert_eq!(create().await.unwrap()["reason"], FAILED_OFFLINE);
    center.online(false, &[CREATE_METHOD]);
    assert_eq!(create().await.unwrap()["reason"], FAILED_OFFLINE);
    center.online(true, &[]);
    assert_eq!(create().await.unwrap()["reason"], FAILED_UNSUPPORTED);
    center.online(true, &[CREATE_METHOD]);
    for (code, reason) in [
        ("automation_unavailable", FAILED_UNAVAILABLE),
        ("limit_reached", FAILED_LIMIT),
        ("forbidden", FAILED_ERROR),
    ] {
        center.answer(Err(CallError::Rejected(code.to_owned())));
        let result = create().await.unwrap();
        assert_eq!(result["state"], "failed");
        assert_eq!(result["reason"], reason);
    }
    center.answer(Ok(
        json!({"endpointId":"e","url":"https://x/y","secret":""}),
    ));
    assert_eq!(create().await.unwrap()["reason"], FAILED_ERROR);
    assert!(external_channels(&bots).await.is_empty());
}

#[tokio::test]
async fn only_webhook_channels_rotate() {
    let fixture = Fixture::new();
    let (bots, center) = bots_with_center(&fixture).await;
    center.online(true, &[CREATE_METHOD]);
    let bot = bots
        .execute("bot.get.request", json!({"workspaceId":WORKSPACE}))
        .await
        .unwrap();
    assert_eq!(
        bots.execute(
            "bot.channel.webhook.rotate.request",
            json!({"workspaceId":WORKSPACE,"channelId":bot["bot"]["builtinChannelId"]}),
        )
        .await,
        Err(ErrorCode::BotConflict)
    );
    assert!(center.calls().is_empty());
}
