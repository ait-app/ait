use super::*;

#[test]
fn rejected_releases_preserve_the_request_id_for_both_budget_categories() {
    for (resource, limit) in [
        ("subscription_releases", 32),
        ("subscription_release_queue", RELEASE_QUEUE_CAPACITY),
    ] {
        let (outbound, mut receiver) = Outbound::new();
        reject_release(&outbound, "release-request", resource, limit).unwrap();
        let queued = receiver.try_recv().unwrap();
        let model::outbound::Frame::Text(text) = &queued.message else {
            panic!("expected a correlated error response");
        };
        let response: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(response["request_id"], "release-request");
        assert_eq!(response["code"], "resource_exhausted");
        assert!(response["retryable"].as_bool().unwrap());
    }
}

#[tokio::test]
async fn full_business_queues_do_not_reject_cleanup_or_block_ping() {
    let fixture = crate::test_support::Workspaces::new(crate::test_support::address());
    let state = &fixture.api.shared;
    let (outbound, mut responses) = Outbound::new();
    let cancel = CancellationToken::new();
    let capabilities = vec![
        "subscription.release.request".to_owned(),
        "connection.ping".to_owned(),
    ];
    let context = WorkerContext {
        state,
        outbound: &outbound,
        peer: Peer {
            capabilities: &capabilities,
            guest: None,
            automation: false,
        },
        cancel: &cancel,
    };
    let channels: [_; 4] = std::array::from_fn(|_| mpsc::channel(WORKER_QUEUE_CAPACITY));
    let (senders, _receivers): (Vec<_>, Vec<_>) = channels.into_iter().unzip();
    let senders: [_; 4] = senders.try_into().unwrap();
    for sender in &senders {
        for _ in 0..WORKER_QUEUE_CAPACITY {
            sender
                .try_send(Work::Message(Incoming::Binary(vec![]), Instant::now()))
                .unwrap();
        }
    }
    let channels: [_; 4] = std::array::from_fn(|_| mpsc::channel(RELEASE_QUEUE_CAPACITY));
    let (cleanup, mut receivers): (Vec<_>, Vec<_>) = channels.into_iter().unzip();
    let cleanup: [_; 4] = cleanup.try_into().unwrap();
    let stream = futures_util::stream::iter([
        serde_json::json!({"type":"request","request_id":"release","method":"subscription.release.request","params":{"subscriptionId":"interest"}}),
        serde_json::json!({"type":"request","request_id":"ping","method":"connection.ping","params":{"nonce":"live"}}),
    ].map(|value| Ok(axum::extract::ws::Message::Text(value.to_string().into()))))
        .chain(futures_util::stream::pending()).boxed();
    let routing = route(stream, &context, senders, cleanup);
    tokio::pin!(routing);
    let check = async {
        let response = responses.recv().await.unwrap();
        let model::outbound::Frame::Text(text) = response.message else {
            panic!("text response");
        };
        let response: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(response["request_id"], "ping");
        let mut acknowledgements = Vec::new();
        for receiver in &mut receivers {
            let Work::Release(id, done) = receiver.recv().await.unwrap() else {
                panic!("cleanup");
            };
            assert_eq!(id, "interest");
            acknowledgements.push(done);
        }
        for done in acknowledgements.drain(..3) {
            done.send(()).unwrap();
        }
        assert!(responses.try_recv().is_err());
        acknowledgements.pop().unwrap().send(()).unwrap();
        let response = responses.recv().await.unwrap();
        let model::outbound::Frame::Text(text) = response.message else {
            panic!("text response");
        };
        let response: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(response["request_id"], "release");
        assert_eq!(response["result"]["subscriptionId"], "interest");
        cancel.cancel();
    };
    tokio::select! { result = &mut routing => panic!("routing ended early: {result:?}"), () = check => {} }
    routing.await.unwrap();
}

#[test]
fn full_worker_discards_only_session_heartbeat() {
    let [
        (metadata, mut metadata_rx),
        (terminal, _terminal_rx),
        (files, _files_rx),
        (provider, _provider_rx),
    ] = std::array::from_fn(|_| mpsc::channel(WORKER_QUEUE_CAPACITY));
    let senders = [metadata, terminal, files, provider];
    let (outbound, mut responses) = Outbound::new();
    let event = |method: &str| {
        Incoming::Text(
            ClientMessage::Event {
                method: method.to_owned(),
                params: serde_json::json!({}),
            },
            None,
        )
    };
    admit(&senders, event(HEARTBEAT_METHOD), &outbound).unwrap();
    assert!(metadata_rx.try_recv().is_ok());
    for _ in 0..WORKER_QUEUE_CAPACITY {
        admit(&senders, event("push.register"), &outbound).unwrap();
    }
    assert_eq!(senders[0].capacity(), 0);

    admit(&senders, event(HEARTBEAT_METHOD), &outbound).unwrap();
    assert!(matches!(
        admit(&senders, event("push.register"), &outbound),
        Err(QueueError::Full)
    ));
    assert!(responses.try_recv().is_err());

    metadata_rx.close();
    assert!(matches!(
        admit(&senders, event(HEARTBEAT_METHOD), &outbound),
        Err(QueueError::Full)
    ));
}

#[test]
fn worker_lanes_keep_streams_and_subscriptions_with_their_connection_owner() {
    for (method, expected) in [
        ("terminal.input", 1),
        ("terminal.list.request", 1),
        ("voice.abort.request", 1),
        ("dictation.stream.start", 1),
        ("checkout.diff.subscribe.request", 2),
        ("file.upload.request", 2),
        ("agent.skills.get_status.request", 2),
        ("agent.skills.reconcile.request", 2),
        ("agent.skills.uninstall.request", 2),
        ("agent.skills.save_selection.request", 2),
        ("agent.skills.import_legacy_selection.request", 2),
        ("workspace.worktree.list.request", 2),
        ("workspace.worktree.create.request", 2),
        ("workspace.worktree.archive.request", 2),
        ("workspace.recovery.inspect.request", 2),
        ("workspace.recovery.restore.request", 2),
        ("workspace.github.search_repositories.request", 2),
        ("project.github.clone.request", 2),
        ("bot.channel.feishu.scan.status.request", 2),
        ("bot.channel.feishu.connect.request", 2),
        ("bot.chat.list.request", 2),
        ("bot.chat.update.request", 2),
        ("bot.context.reset.request", 2),
        ("bot.channel.lark.connect.request", 2),
        ("bot.channel.telegram.pair.status.request", 2),
        ("bot.channel.weixin.scan.status.request", 2),
        ("bot.channel.qq.scan.status.request", 2),
        ("bot.channel.imessage.pair.status.request", 2),
        ("bot.channel.create.request", 0),
        ("agent.list.request", 3),
        ("agent.timeline.set_subscription.request", 3),
        ("workspace.create.request", 0),
        ("workspace.archive.request", 0),
        ("project.add.request", 0),
        ("schedule.list.request", 0),
        ("browser.host.register.request", 0),
        ("relay.status.request", 0),
        ("unknown.request", 0),
        ("agent.skills.unknown.request", 0),
        ("workspace.worktree.unknown.request", 0),
        ("project.github.unknown.request", 0),
    ] {
        let message = Incoming::Text(
            ClientMessage::Request {
                request_id: "r1".to_owned(),
                method: method.to_owned(),
                params: serde_json::json!({}),
            },
            None,
        );
        assert_eq!(lane(&message), expected, "{method}");
    }
    assert_eq!(lane(&Incoming::Binary(vec![0x01])), 1);
    assert_eq!(lane(&Incoming::Binary(vec![0x10])), 2);
    assert_eq!(lane(&Incoming::Binary(Vec::new())), 2);
}
