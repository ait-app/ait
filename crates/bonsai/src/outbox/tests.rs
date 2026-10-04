use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::time::Instant;

use super::{Bucket, LastFrame, Outbox, Outgoing, Sink, Writer};
use crate::ports::BoxFuture;
use crate::store::Store;
use crate::testing::dispatch;
use crate::wire::{Cursor, RunState, Status};

#[derive(Clone, Default)]
struct Recorder {
    sent: Arc<Mutex<Vec<(Instant, String)>>>,
    /// How long each send takes (a slow uplink).
    delay: Duration,
}

impl Recorder {
    fn slow(delay: Duration) -> Self {
        Self {
            delay,
            ..Self::default()
        }
    }

    /// Times at which a heartbeat `ping` went out.
    fn pings(&self) -> Vec<Instant> {
        self.sent
            .lock()
            .expect("lock")
            .iter()
            .filter(|(_, text)| text == "ping")
            .map(|(at, _)| *at)
            .collect()
    }

    /// Every frame but heartbeats, in order, with the time it went out.
    fn frames(&self) -> Vec<(Instant, Value, Vec<Value>)> {
        self.sent
            .lock()
            .expect("lock")
            .iter()
            .filter(|(_, text)| text != "ping")
            .map(|(at, text)| {
                let (head, body) = text.split_once('\n').unwrap_or((text, "[]"));
                (
                    *at,
                    serde_json::from_str(head).expect("head"),
                    serde_json::from_str(body).expect("body"),
                )
            })
            .collect()
    }
}

impl Sink for Recorder {
    fn send(&mut self, text: String) -> BoxFuture<'_, Result<(), ()>> {
        Box::pin(async move {
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            self.sent.lock().expect("lock").push((Instant::now(), text));
            Ok(())
        })
    }

    fn close(&mut self, code: u16) -> BoxFuture<'_, Result<(), ()>> {
        self.sent.lock().expect("lock").push((
            Instant::now(),
            format!("{{\"type\":\"close\",\"code\":{code}}}"),
        ));
        Box::pin(async { Ok(()) })
    }
}

fn store_with(runs: &[&str]) -> Store {
    let store = Store::memory().expect("store");
    for run in runs {
        store.insert_run(&dispatch(run), "e1", 0).expect("insert");
    }
    store
}

fn append(store: &Store, run: &str, count: u64) {
    let record = store.run(run).expect("read").expect("run");
    let events: Vec<String> = (0..count)
        .map(|offset| {
            json!({"seq": record.next_seq + offset, "at": 0, "t": "text", "mid": "m", "text": "x"})
                .to_string()
        })
        .collect();
    store
        .append(run, &record.epoch, record.next_seq, &events, None)
        .expect("append");
}

fn start(
    store: &Store,
) -> (
    Recorder,
    mpsc::UnboundedSender<Outgoing>,
    tokio::task::JoinHandle<()>,
) {
    let (recorder, sender, task, _) = start_with(store, Recorder::default());
    (recorder, sender, task)
}

fn start_with(
    store: &Store,
    recorder: Recorder,
) -> (
    Recorder,
    mpsc::UnboundedSender<Outgoing>,
    tokio::task::JoinHandle<()>,
    Arc<Mutex<LastFrame>>,
) {
    let last = Arc::new(Mutex::new(LastFrame::default()));
    let writer = Writer::new(recorder.clone(), store.clone(), Arc::clone(&last)).expect("writer");
    let (sender, receiver) = mpsc::unbounded_channel();
    let task = tokio::spawn(writer.run(receiver));
    (recorder, sender, task, last)
}

/// The status bucket's burst (it keeps headroom under the Hub's 20).
const BURST: usize = 15;

fn status(run_id: &str) -> Outgoing {
    status_in(run_id, RunState::Cancelled)
}

fn status_in(run_id: &str, state: RunState) -> Outgoing {
    Outgoing::Status(Status {
        kind: "run.status",
        run_id: run_id.to_owned(),
        status: state,
        at: 1,
        execution: None,
        reason_code: None,
        reason_detail: None,
        final_text: None,
    })
}

#[tokio::test(start_paused = true)]
async fn concurrent_streams_share_one_live_budget_without_losing_events() {
    let runs = ["r_a", "r_b", "r_c", "r_d", "r_e", "r_f"];
    let store = store_with(&runs);
    let (recorder, sender, _task) = start(&store);
    // Every run streams a few events every 150 ms for 10 seconds.
    for _ in 0..67 {
        for run in runs {
            append(&store, run, 3);
            sender
                .send(Outgoing::Live {
                    run_id: run.to_owned(),
                })
                .expect("queue");
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    tokio::time::sleep(Duration::from_secs(5)).await;
    let frames = recorder.frames();
    // Replay the Hub's realtime bucket (20/s, burst 100): never short of a token.
    let mut tokens = 100.0_f64;
    let mut previous: Option<Instant> = None;
    for (at, head, _) in &frames {
        assert!(head.get("sub").is_none());
        if let Some(previous) = previous {
            tokens = (tokens + at.duration_since(previous).as_secs_f64() * 20.0).min(100.0);
        }
        previous = Some(*at);
        tokens -= 1.0;
        assert!(tokens >= 0.0, "a live frame would be dropped by the Hub");
    }
    let span = frames
        .last()
        .zip(frames.first())
        .map(|(last, first)| last.0.duration_since(first.0).as_secs_f64())
        .unwrap_or_default();
    let count = f64::from(u32::try_from(frames.len()).expect("a small count"));
    assert!(count <= span * 15.0 + 2.0, "over the per-connection budget");
    for run in runs {
        let seqs: Vec<u64> = frames
            .iter()
            .filter(|(_, head, _)| head["run_id"] == run)
            .flat_map(|(_, _, body)| body.iter().map(|event| event["seq"].as_u64().expect("seq")))
            .collect();
        assert_eq!(
            seqs,
            (0..201).collect::<Vec<_>>(),
            "{run} lost or reordered events"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn answers_flush_live_first_and_mark_reset_and_sync() {
    let store = store_with(&["r_1"]);
    let (recorder, sender, _task) = start(&store);
    append(&store, "r_1", 5);
    sender
        .send(Outgoing::Answer {
            run_id: "r_1".to_owned(),
            sub: "s".repeat(32),
            after: None,
        })
        .expect("queue");
    tokio::time::sleep(Duration::from_millis(10)).await;
    let frames = recorder.frames();
    assert_eq!(frames.len(), 2, "one live flush, then one answer");
    assert!(frames[0].1.get("sub").is_none());
    assert_eq!(
        (frames[0].1["first"].as_u64(), frames[0].1["last"].as_i64()),
        (Some(0), Some(4))
    );
    let answer = &frames[1].1;
    assert_eq!(answer["sub"], "s".repeat(32));
    assert_eq!(answer["reset"], true);
    assert_eq!(answer["sync"], true);
    assert_eq!(frames[1].2.len(), 5);
}

#[tokio::test(start_paused = true)]
async fn answers_continue_after_a_cursor_or_restart_on_a_stale_one() {
    let store = store_with(&["r_1"]);
    let (recorder, sender, _task) = start(&store);
    append(&store, "r_1", 4);
    let cases = [
        (
            Some(Cursor {
                epoch: "e1".to_owned(),
                seq: 1,
            }),
            Some(2),
            false,
            2,
        ),
        (
            Some(Cursor {
                epoch: "e1".to_owned(),
                seq: 3,
            }),
            Some(4),
            false,
            0,
        ),
        (
            Some(Cursor {
                epoch: "old".to_owned(),
                seq: 1,
            }),
            Some(0),
            true,
            4,
        ),
        (
            Some(Cursor {
                epoch: "e1".to_owned(),
                seq: 9,
            }),
            Some(0),
            true,
            4,
        ),
        (
            Some(Cursor {
                epoch: "e1".to_owned(),
                seq: -2,
            }),
            Some(0),
            true,
            4,
        ),
        (
            Some(Cursor {
                epoch: "e1".to_owned(),
                seq: -1,
            }),
            Some(0),
            false,
            4,
        ),
    ];
    for (after, first, reset, count) in cases {
        recorder.sent.lock().expect("lock").clear();
        sender
            .send(Outgoing::Answer {
                run_id: "r_1".to_owned(),
                sub: "a".repeat(32),
                after: after.clone(),
            })
            .expect("queue");
        tokio::time::sleep(Duration::from_millis(10)).await;
        let frames = recorder.frames();
        let (_, head, body) = frames.last().expect("an answer");
        assert_eq!(head["first"].as_u64(), first, "{after:?}");
        assert_eq!(head.get("reset").is_some(), reset, "{after:?}");
        assert_eq!(head["sync"], true);
        assert_eq!(body.len(), count, "{after:?}");
    }
}

#[tokio::test(start_paused = true)]
async fn unknown_runs_get_no_history_and_unavailable_refs_go_out() {
    let store = store_with(&[]);
    let (recorder, sender, _task) = start(&store);
    sender
        .send(Outgoing::Answer {
            run_id: "r_x".to_owned(),
            sub: "b".repeat(32),
            after: None,
        })
        .expect("queue");
    sender
        .send(Outgoing::Unavailable {
            run_id: "r_x".to_owned(),
            reference: "input-1".to_owned(),
        })
        .expect("queue");
    tokio::time::sleep(Duration::from_millis(10)).await;
    let frames = recorder.frames();
    assert_eq!(frames[0].1["type"], "session.unavailable");
    assert_eq!(frames[0].1["sub"], "b".repeat(32));
    assert!(frames[0].1.get("ref").is_none());
    assert_eq!(frames[1].1["ref"], "input-1");
    assert!(frames[1].1.get("sub").is_none());
}

#[tokio::test(start_paused = true)]
async fn events_logged_while_disconnected_are_not_resent_live() {
    let store = store_with(&["r_1"]);
    append(&store, "r_1", 3);
    let (recorder, sender, _task) = start(&store);
    append(&store, "r_1", 2);
    sender
        .send(Outgoing::Live {
            run_id: "r_1".to_owned(),
        })
        .expect("queue");
    tokio::time::sleep(Duration::from_millis(10)).await;
    let frames = recorder.frames();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].1["first"], 3);
    assert_eq!(frames[0].2.len(), 2);
}

#[tokio::test(start_paused = true)]
async fn epoch_rotation_broadcasts_an_empty_frame_and_moves_the_pointer() {
    let store = store_with(&["r_1"]);
    let (recorder, sender, _task) = start(&store);
    append(&store, "r_1", 2);
    store.rotate("r_1", "e2").expect("rotate");
    append(&store, "r_1", 3);
    sender
        .send(Outgoing::Epoch {
            run_id: "r_1".to_owned(),
            epoch: "e2".to_owned(),
            next: 3,
        })
        .expect("queue");
    append(&store, "r_1", 1);
    sender
        .send(Outgoing::Live {
            run_id: "r_1".to_owned(),
        })
        .expect("queue");
    tokio::time::sleep(Duration::from_secs(1)).await;
    let frames = recorder.frames();
    assert_eq!(frames[0].1["epoch"], "e2");
    assert_eq!(
        (frames[0].1["first"].as_i64(), frames[0].1["last"].as_i64()),
        (Some(0), Some(-1))
    );
    assert_eq!(frames[1].1["first"], 3);
    assert_eq!(frames[1].2.len(), 1);
}

#[tokio::test(start_paused = true)]
async fn status_frames_pass_through_their_own_bucket() {
    let store = store_with(&["r_1"]);
    let (recorder, sender, _task) = start(&store);
    for _ in 0..22 {
        sender
            .send(Outgoing::Status(Status {
                kind: "run.status",
                run_id: "r_1".to_owned(),
                status: RunState::Cancelled,
                at: 1,
                execution: None,
                reason_code: None,
                reason_detail: None,
                final_text: None,
            }))
            .expect("queue");
    }
    tokio::time::sleep(Duration::from_secs(5)).await;
    let frames = recorder.frames();
    assert_eq!(frames.len(), 22);
    let burst_end = frames[19].0;
    assert!(frames[21].0.duration_since(burst_end) >= Duration::from_millis(900));
}

#[tokio::test(start_paused = true)]
async fn statuses_the_hub_asked_for_are_not_counted() {
    // Arrange: 30 runs queried after a reconnect.
    let runs: Vec<String> = (0..30).map(|index| format!("r_{index}")).collect();
    let store = Store::memory().expect("store");
    let (recorder, sender, _task) = start(&store);

    // Act
    for run in &runs {
        sender
            .send(Outgoing::Credit {
                run_id: run.clone(),
            })
            .expect("queue");
        sender.send(status(run)).expect("queue");
    }
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Assert: all 30 at once, beyond the burst of 20.
    assert_eq!(recorder.frames().len(), 30);
}

#[tokio::test(start_paused = true)]
async fn waiting_statuses_never_hold_up_other_frames() {
    // Arrange
    let store = store_with(&["r_1"]);
    let (recorder, sender, _task) = start(&store);
    for _ in 0..25 {
        sender.send(status("r_2")).expect("queue");
    }

    // Act: events arrive while five statuses wait for the bucket.
    append(&store, "r_1", 1);
    sender
        .send(Outgoing::Live {
            run_id: "r_1".to_owned(),
        })
        .expect("queue");
    tokio::time::sleep(Duration::from_millis(10)).await;
    let early = recorder.frames();
    tokio::time::sleep(Duration::from_secs(10)).await;

    // Assert
    assert_eq!(
        early.len(),
        BURST + 1,
        "a burst of statuses and the live frame"
    );
    assert_eq!(early[BURST].1["type"], "session.events");
    assert_eq!(recorder.frames().len(), 26);
}

#[tokio::test(start_paused = true)]
async fn an_asked_status_replaces_older_waiting_ones_of_its_run() {
    // Arrange: the burst is spent, two statuses of r_1 wait.
    let store = Store::memory().expect("store");
    let (recorder, sender, _task) = start(&store);
    for _ in 0..BURST {
        sender.send(status("r_0")).expect("queue");
    }
    sender.send(status("r_1")).expect("queue");
    sender.send(status("r_1")).expect("queue");

    // Act
    sender
        .send(Outgoing::Credit {
            run_id: "r_1".to_owned(),
        })
        .expect("queue");
    sender.send(status("r_1")).expect("queue");
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Assert: the burst plus the asked one; the two waiting ones are superseded.
    let frames = recorder.frames();
    assert_eq!(frames.len(), BURST + 1);
    assert_eq!(frames[BURST].1["run_id"], "r_1");
}

#[tokio::test(start_paused = true)]
async fn an_asked_status_never_drops_a_newer_one_that_waits() {
    // Arrange: the burst is spent; the run completed and that status waits.
    let store = Store::memory().expect("store");
    let (recorder, sender, _task) = start(&store);
    for _ in 0..BURST {
        sender.send(status("r_0")).expect("queue");
    }
    sender
        .send(status_in("r_1", RunState::Completed))
        .expect("queue");

    // Act: the Hub's query was answered from an older read.
    sender
        .send(Outgoing::Credit {
            run_id: "r_1".to_owned(),
        })
        .expect("queue");
    sender
        .send(status_in("r_1", RunState::Running))
        .expect("queue");
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Assert: the asked one at once, the waiting completed one after it.
    let states: Vec<String> = recorder
        .frames()
        .iter()
        .filter(|frame| frame.1["run_id"] == "r_1")
        .map(|frame| frame.1["status"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(states, ["running", "completed"]);
}

#[tokio::test(start_paused = true)]
async fn the_heartbeat_goes_out_between_the_frames_of_a_long_answer() {
    // Arrange: 14 frames of replay (three 60 KB events each) on an uplink that takes three
    // seconds per frame.
    let store = store_with(&["r_1"]);
    let record = store.run("r_1").expect("read").expect("run");
    let events: Vec<String> = (0..40)
        .map(|seq| {
            json!({"seq": seq, "at": 0, "t": "text", "mid": "m", "text": "y".repeat(60_000)})
                .to_string()
        })
        .collect();
    store
        .append("r_1", &record.epoch, 0, &events, None)
        .expect("append");
    let (recorder, sender, _task, last) =
        start_with(&store, Recorder::slow(Duration::from_secs(3)));

    // Act
    sender
        .send(Outgoing::Answer {
            run_id: "r_1".to_owned(),
            sub: "s".to_owned(),
            after: None,
        })
        .expect("queue");
    tokio::time::sleep(Duration::from_mins(1)).await;

    // Assert
    let frames = recorder.frames();
    assert_eq!(frames.len(), 14);
    let pings = recorder.pings();
    assert!(!pings.is_empty(), "a ping went out during the answer");
    assert!(pings[0] < frames[13].0, "before the answer ended");
    let last_frame = last.lock().expect("lock").clone();
    assert_eq!(
        last_frame.range,
        Some((39, 39)),
        "each frame records its own range"
    );
}

#[test]
fn outbox_drops_frames_without_a_connection() {
    let outbox = Outbox::default();
    outbox.send(Outgoing::Live {
        run_id: "r".to_owned(),
    });
    let (sender, mut receiver) = mpsc::unbounded_channel();
    outbox.attach(sender);
    outbox.send(Outgoing::Live {
        run_id: "r".to_owned(),
    });
    assert!(receiver.try_recv().is_ok());
    outbox.detach();
    outbox.send(Outgoing::Live {
        run_id: "r".to_owned(),
    });
    assert!(receiver.try_recv().is_err());
}

#[tokio::test(start_paused = true)]
async fn buckets_refill_at_their_rate() {
    let mut bucket = Bucket::new(2.0, 1.0);
    assert!(bucket.wait().is_zero());
    bucket.take();
    assert_eq!(bucket.wait(), Duration::from_millis(500));
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(bucket.wait().is_zero());
}

#[tokio::test(start_paused = true)]
async fn finished_runs_only_send_what_is_appended_after_connecting() {
    let store = store_with(&["r_done"]);
    append(&store, "r_done", 4);
    let update = crate::store::StatusUpdate {
        status: RunState::Completed,
        reason_code: None,
        reason_detail: None,
        final_text: None,
        at: 1,
    };
    store.advance("r_done", &update).expect("finish");
    let (recorder, sender, _task) = start(&store);
    append(&store, "r_done", 1);
    sender
        .send(Outgoing::Live {
            run_id: "r_done".to_owned(),
        })
        .expect("queue");
    tokio::time::sleep(Duration::from_millis(10)).await;
    let frames = recorder.frames();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].1["first"], 4, "the old log is not replayed live");
}
