use serde_json::json;
use tokio::task::JoinHandle;

use super::*;

/// An opener on a connected link that enabled `peer`, driven without a socket:
/// frames it queues collect in `frames`.
struct Harness {
    opener: Opener,
    caller: Caller,
    link: Link,
    frames: Vec<Message>,
    state: watch::Sender<LinkState>,
}

fn harness() -> Harness {
    let (state, link_state) = watch::channel(LinkState {
        online: true,
        ..LinkState::default()
    });
    let (caller, mut opener) = Opener::channel(link_state);
    opener.joined(true, true);
    Harness {
        opener,
        caller,
        link: Link::new(Vec::new()),
        frames: Vec::new(),
        state,
    }
}

fn request(tool: &str, arguments: Value) -> Request {
    Request {
        tool: tool.to_owned(),
        arguments,
        caller: None,
    }
}

/// A request of about 40 KB: two do not fit one 64 KiB window.
fn large() -> Request {
    request("agent_prompt", json!({"text":"x".repeat(40_000)}))
}

impl Harness {
    /// Start a call from a task, as the `Connector` does.
    fn call(
        &self,
        node: Uuid,
        request: Request,
        limit: Duration,
    ) -> JoinHandle<Result<Value, Error>> {
        let caller = self.caller.clone();
        tokio::spawn(async move { caller.call(node, request, limit).await })
    }

    /// Hand the next queued call to the opener, as the link loop does.
    async fn take(&mut self) {
        let call = self.opener.queue.recv().await.expect("a queued call");
        self.opener
            .call(call, &mut self.link, &mut self.frames)
            .expect("frames encode");
    }

    /// The frames queued so far, as (header, payload).
    fn written(&mut self) -> Vec<(Value, Vec<u8>)> {
        self.frames
            .drain(..)
            .map(|message| {
                let Message::Binary(bytes) = message else {
                    panic!("hub frames are binary");
                };
                let length = usize::from(u16::from_be_bytes([bytes[0], bytes[1]]));
                let header = serde_json::from_slice(&bytes[2..2 + length]).expect("header");
                (header, bytes[2 + length..].to_vec())
            })
            .collect()
    }

    /// The session of the single `open` written since the last look.
    fn opened(&mut self) -> Uuid {
        let written = self.written();
        assert_eq!(written.len(), 1, "{written:?}");
        assert_eq!(written[0].0["t"], "open");
        serde_json::from_value(written[0].0["s"].clone()).expect("session id")
    }

    /// The ids of the requests written since the last look; nothing else was written.
    fn sent(&mut self) -> Vec<u64> {
        self.written()
            .into_iter()
            .map(|(header, payload)| {
                assert_eq!(header["t"], "msg", "{header}");
                let request: Value = serde_json::from_slice(&payload).expect("request");
                request["id"].as_u64().expect("id")
            })
            .collect()
    }

    fn accept(&mut self, s: Uuid, window: Option<u32>) {
        let ours = self
            .opener
            .accepted(s, window, HELLO, &mut self.link, &mut self.frames)
            .expect("frames encode");
        assert!(ours);
    }

    fn reply(&mut self, s: Uuid, reply: &Value) {
        let ours = self
            .opener
            .message(
                s,
                PayloadKind::Text,
                reply.to_string().as_bytes(),
                &mut self.link,
                &mut self.frames,
            )
            .expect("frames encode");
        assert!(ours);
    }

    /// An accepted session to `node` with `window` whose first request, `first`, was sent.
    async fn sending(
        &mut self,
        node: Uuid,
        first: Request,
        window: Option<u32>,
    ) -> (Uuid, JoinHandle<Result<Value, Error>>) {
        let call = self.call(node, first, Duration::from_secs(60));
        self.take().await;
        let s = self.opened();
        self.accept(s, window);
        assert_eq!(self.sent(), [1]);
        (s, call)
    }
}

#[test]
fn a_call_given_up_at_its_limit_says_how_far_it_got() {
    let at = |stage: Stage| {
        let progress = Progress::queued();
        assert!(progress.reach(stage));
        progress.give_up()
    };
    assert_eq!(at(Stage::Queued), Stage::Queued);
    assert_eq!(at(Stage::Opening), Stage::Opening);
    assert_eq!(at(Stage::Held), Stage::Held);
    assert_eq!(at(Stage::Sent), Stage::Sent);
    for online in [true, false] {
        assert_eq!(Stage::Sent.at_limit(online), Error::OutcomeUnknown);
        assert_eq!(Stage::Opening.at_limit(online), Error::Offline);
    }
    assert_eq!(Stage::Queued.at_limit(true), Error::Capacity);
    assert_eq!(Stage::Held.at_limit(true), Error::Capacity);
    assert_eq!(Stage::Queued.at_limit(false), Error::Unavailable);
    assert_eq!(Stage::Held.at_limit(false), Error::Unavailable);
    // A call never moves back.
    let progress = Progress::queued();
    assert!(progress.reach(Stage::Held));
    assert!(progress.reach(Stage::Opening));
    assert_eq!(progress.stage(), Stage::Held);
}

#[test]
fn a_call_given_up_is_never_sent() {
    let progress = Progress::queued();
    assert!(progress.reach(Stage::Opening));
    assert_eq!(progress.give_up(), Stage::Opening);
    assert!(!progress.reach(Stage::Held));
    assert!(!progress.reach(Stage::Sent));
    assert_eq!(progress.stage(), Stage::GaveUp);
}

#[tokio::test(start_paused = true)]
async fn a_call_given_up_before_accept_is_not_sent_once_accepted() {
    let mut harness = harness();
    // Its caller is still waiting on the answer: only the shared stage says it gave up.
    let (reply, _answer) = oneshot::channel();
    let progress = Progress::queued();
    let call = Call {
        node: Uuid::new_v4(),
        request: request("agent_create", json!({})),
        reply,
        progress: progress.clone(),
    };
    harness
        .opener
        .call(call, &mut harness.link, &mut harness.frames)
        .expect("frames encode");
    let s = harness.opened();
    assert_eq!(progress.give_up(), Stage::Opening);

    harness.accept(s, None);
    assert_eq!(harness.sent(), Vec::<u64>::new());
    assert!(harness.opener.sessions[&s].sent.is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_new_logical_link_leaves_sent_requests_unknown_and_held_ones_unavailable() {
    let mut harness = harness();
    let node = Uuid::new_v4();
    let (_, sent) = harness.sending(node, large(), Some(65_536)).await;
    let held = harness.call(node, large(), Duration::from_secs(60));
    harness.take().await;
    assert_eq!(harness.sent(), Vec::<u64>::new(), "the window holds it");

    harness.opener.joined(true, true);
    assert_eq!(sent.await.expect("call"), Err(Error::OutcomeUnknown));
    assert_eq!(held.await.expect("call"), Err(Error::Unavailable));
}

#[tokio::test(start_paused = true)]
async fn a_link_that_ends_leaves_sent_requests_unknown() {
    let mut harness = harness();
    let (_, sent) = harness
        .sending(Uuid::new_v4(), request("agent_create", json!({})), None)
        .await;
    drop(harness.opener);
    assert_eq!(sent.await.expect("call"), Err(Error::OutcomeUnknown));
}

#[tokio::test(start_paused = true)]
async fn an_answer_dropped_unsent_is_decided_by_how_far_the_call_got() {
    let mut harness = harness();
    let node = Uuid::new_v4();
    let (_, sent) = harness.sending(node, large(), Some(65_536)).await;
    let held = harness.call(node, large(), Duration::from_secs(60));
    harness.take().await;
    // Drop the sessions without answering anyone, as a bug in ending them would.
    harness.opener.sessions.clear();
    assert_eq!(sent.await.expect("call"), Err(Error::OutcomeUnknown));
    assert_eq!(held.await.expect("call"), Err(Error::Unavailable));
}

#[tokio::test(start_paused = true)]
async fn an_idle_peer_session_is_closed_by_the_opener() {
    let mut harness = harness();
    let (s, call) = harness
        .sending(Uuid::new_v4(), request("agent_list", json!({})), None)
        .await;
    assert_eq!(harness.opener.next_idle(), None, "a request is outstanding");
    harness.reply(s, &json!({"id":1,"result":{}}));
    assert_eq!(call.await.expect("call"), Ok(json!({})));
    assert_eq!(
        harness.opener.next_idle(),
        Some(Instant::now() + Duration::from_secs(60))
    );

    tokio::time::advance(Duration::from_secs(59)).await; // virtual time
    harness
        .opener
        .expire(&mut harness.link, &mut harness.frames)
        .expect("frames encode");
    assert!(harness.written().is_empty());
    tokio::time::advance(Duration::from_secs(1)).await; // virtual time
    harness
        .opener
        .expire(&mut harness.link, &mut harness.frames)
        .expect("frames encode");
    assert_eq!(
        harness.written(),
        [(json!({"t":"close","seq":3,"s":s,"code":"done"}), Vec::new())]
    );
    assert_eq!(harness.opener.next_idle(), None);
}

#[tokio::test(start_paused = true)]
async fn a_call_whose_open_is_never_accepted_is_offline_at_its_limit() {
    let mut harness = harness();
    let call = harness.call(
        Uuid::new_v4(),
        request("agent_list", json!({})),
        Duration::from_secs(10),
    );
    harness.take().await;
    let s = harness.opened();

    tokio::time::advance(Duration::from_secs(9)).await; // virtual time
    tokio::task::yield_now().await;
    assert!(!call.is_finished(), "the call waits until its limit");
    tokio::time::advance(Duration::from_secs(1)).await; // virtual time
    assert_eq!(call.await.expect("call"), Err(Error::Offline));
    // Nothing was sent, and the open is not left behind.
    harness
        .opener
        .prune(&mut harness.link, &mut harness.frames)
        .expect("frames encode");
    assert_eq!(
        harness.written(),
        [(json!({"t":"close","seq":2,"s":s,"code":"done"}), Vec::new())]
    );
}

#[tokio::test(start_paused = true)]
async fn a_sent_call_is_outcome_unknown_at_its_limit_and_its_late_reply_is_ignored() {
    let mut harness = harness();
    let node = Uuid::new_v4();
    let limited = harness.call(
        node,
        request("agent_wait", json!({"agentId":"b-7"})),
        Duration::from_secs(10),
    );
    harness.take().await;
    let s = harness.opened();
    harness.accept(s, Some(credit::WINDOW));
    let second = harness.call(
        node,
        request("agent_get", json!({"agentId":"b-8"})),
        Duration::from_secs(60),
    );
    harness.take().await;
    assert_eq!(harness.sent(), [1, 2]);

    tokio::time::advance(Duration::from_secs(10)).await; // virtual time
    assert_eq!(limited.await.expect("call"), Err(Error::OutcomeUnknown));
    harness
        .opener
        .prune(&mut harness.link, &mut harness.frames)
        .expect("frames encode");

    // The late reply reaches nobody but is credited; the second call gets its own.
    let late = json!({"id":1,"result":{"content":["late"]}});
    let answer = json!({"id":2,"result":{"structuredContent":{"agent":"b-8"}}});
    harness.reply(s, &late);
    harness.reply(s, &answer);
    assert_eq!(
        second.await.expect("call"),
        Ok(json!({"structuredContent":{"agent":"b-8"}}))
    );
    let late_length = late.to_string().len() as u64;
    let total = late_length + answer.to_string().len() as u64;
    assert_eq!(
        harness.written(),
        [
            (json!({"t":"credit","s":s,"c":late_length}), Vec::new()),
            (json!({"t":"credit","s":s,"c":total}), Vec::new()),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn a_call_waiting_for_the_window_is_capacity_at_its_limit_and_never_sent() {
    let mut harness = harness();
    let node = Uuid::new_v4();
    let (s, _first) = harness.sending(node, large(), Some(65_536)).await;
    let waiting = harness.call(node, large(), Duration::from_secs(10));
    harness.take().await;
    assert_eq!(harness.sent(), Vec::<u64>::new(), "the window holds it");

    tokio::time::advance(Duration::from_secs(10)).await; // virtual time
    assert_eq!(waiting.await.expect("call"), Err(Error::Capacity));
    // Credit that would admit it now sends nothing.
    let credited = harness
        .opener
        .credit(s, 65_536, &mut harness.link, &mut harness.frames)
        .expect("frames encode");
    assert!(credited);
    assert_eq!(harness.sent(), Vec::<u64>::new());
}

#[tokio::test(start_paused = true)]
async fn a_call_that_waits_out_a_link_outage_is_unavailable() {
    let harness = harness();
    // The link loop drains no calls, as while the link is not connected.
    for (online, error) in [(false, Error::Unavailable), (true, Error::Capacity)] {
        harness.state.send_modify(|state| state.online = online);
        let call = harness.call(
            Uuid::new_v4(),
            request("agent_list", json!({})),
            Duration::from_secs(10),
        );
        // Let the call start its clock and wait in the queue.
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(9)).await; // virtual time
        assert!(!call.is_finished(), "the call waits until its limit");
        tokio::time::advance(Duration::from_secs(1)).await; // virtual time
        assert_eq!(call.await.expect("call"), Err(error), "online: {online}");
    }
}

#[tokio::test(start_paused = true)]
async fn a_request_over_the_payload_limit_is_refused_without_an_open() {
    let mut harness = harness();
    let oversized = request(
        "agent_prompt",
        json!({"text":"x".repeat(wire::MAX_PAYLOAD)}),
    );
    let call = harness.call(Uuid::new_v4(), oversized, Duration::from_secs(60));
    harness.take().await;

    assert_eq!(call.await.expect("call"), Err(Error::RequestTooLarge));
    assert!(harness.written().is_empty());
    assert!(harness.opener.sessions.is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_request_larger_than_the_window_is_refused_unsent() {
    let mut harness = harness();
    let node = Uuid::new_v4();
    let (s, _first) = harness
        .sending(node, request("agent_list", json!({})), Some(65_536))
        .await;
    let over_window = request("agent_prompt", json!({"text":"x".repeat(70_000)}));
    let call = harness.call(node, over_window, Duration::from_secs(60));
    harness.take().await;

    assert_eq!(call.await.expect("call"), Err(Error::RequestTooLarge));
    assert_eq!(harness.sent(), Vec::<u64>::new());
    assert_eq!(harness.opener.sessions[&s].sent.len(), 1, "only the first");
}

#[test]
fn a_close_before_sending_maps_to_why_the_call_did_not_run() {
    for (code, error) in [
        (Some("unknown_target"), Error::Offline),
        (Some("peer_gone"), Error::Offline),
        (Some("unsupported"), Error::Unsupported),
        (Some("peer_disabled"), Error::NotAccepting),
        (Some("capacity"), Error::Capacity),
        (Some("replaced"), Error::Unavailable),
        (Some("something_new"), Error::Unavailable),
        (None, Error::Unavailable),
    ] {
        assert_eq!(refusal(code), error, "{code:?}");
    }
}

#[tokio::test(start_paused = true)]
async fn a_target_gone_before_its_request_was_sent_is_offline() {
    let mut harness = harness();
    let call = harness.call(
        Uuid::new_v4(),
        request("agent_list", json!({})),
        Duration::from_secs(60),
    );
    harness.take().await;
    let s = harness.opened();

    assert!(harness.opener.closed(s, Some("peer_gone")));
    assert_eq!(call.await.expect("call"), Err(Error::Offline));
}
