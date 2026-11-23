use std::io::Write;
use std::process::{Command, Stdio};

use super::*;

/// A synthetic transcript for the harness's own tests, not a recording of any real CLI.
const SYNTHETIC: &str = r#"{"type":"meta","format":"ait-stdio-replay","version":1,"provider":"synthetic"}
{"type":"spawn","process":0,"argv":["serve"]}
{"type":"stdout","process":0,"frame":{"method":"ready","params":{}}}
{"type":"stdin","process":0,"frame":{"id":0,"method":"open","params":{"cwd":"{{cwd}}","client":"{{client}}","nonce":"{{any}}"}}}
{"type":"stdout","process":0,"frame":{"id":0,"result":{"session":"s-1"}}}
{"type":"stdout","process":0,"frame":{"id":"ask-1","method":"approve","params":{}}}
{"type":"stdin","process":0,"frame":{"id":"ask-1","result":{"allow":true}}}
{"type":"stdout","process":0,"frame":{"method":"done","params":{"session":"s-1"}}}
"#;

fn synthetic_peer() -> (tempfile::TempDir, StdioPeer) {
    let directory = tempfile::tempdir().unwrap();
    let transcript = directory.path().join("synthetic.jsonl");
    std::fs::write(&transcript, SYNTHETIC).unwrap();
    let peer = StdioPeer::replay("cli", &transcript, &[("{{client}}", "test-client")]);
    (directory, peer)
}

/// Launch the peer, write every line of `input`, close stdin, and return the exit status and
/// the frames it wrote.
fn exchange(peer: &StdioPeer, argv: &[&str], input: &[Value]) -> (bool, Vec<Value>) {
    let mut child = Command::new(peer.program())
        .args(argv)
        .current_dir(peer.cwd())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for frame in input {
        // A peer that already failed closes its stdin; the status below reports that.
        let _ = writeln!(stdin, "{frame}");
    }
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    let frames = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    (output.status.success(), frames)
}

#[test]
fn replay_answers_in_recorded_order_with_the_adapter_request_ids() {
    let (_directory, peer) = synthetic_peer();
    let open = json!({"id":41,"method":"open","params":{
        "cwd":peer.cwd(),"client":"test-client","nonce":"random"}});
    let answer = json!({"id":"ask-1","result":{"allow":true}});

    let (success, frames) = exchange(&peer, &["serve"], &[open, answer]);

    assert!(success);
    assert_eq!(
        frames,
        [
            json!({"method":"ready","params":{}}),
            json!({"id":41,"result":{"session":"s-1"}}),
            json!({"id":"ask-1","method":"approve","params":{}}),
            json!({"method":"done","params":{"session":"s-1"}}),
        ]
    );
    assert_eq!(peer.mismatch(), None);
    peer.assert_replayed();
}

#[test]
fn replay_stops_at_the_first_frame_the_recording_does_not_expect() {
    let (_directory, peer) = synthetic_peer();
    let open = json!({"id":0,"method":"open","params":{
        "cwd":"/elsewhere","client":"test-client","nonce":"random"}});

    let (success, frames) = exchange(&peer, &["serve"], &[open]);

    assert!(!success);
    assert_eq!(frames, [json!({"method":"ready","params":{}})]);
    let mismatch = peer.mismatch().unwrap();
    assert_eq!(mismatch["mismatch"], "frame differs from the recording");
    assert_eq!(mismatch["actual"]["params"]["cwd"], "/elsewhere");
    assert_eq!(mismatch["expected"]["params"]["cwd"], peer.cwd());
}

#[test]
fn replay_rejects_other_methods_launch_arguments_and_extra_launches() {
    let (_directory, peer) = synthetic_peer();

    let (success, _) = exchange(&peer, &["serve"], &[json!({"id":0,"method":"close"})]);
    assert!(!success);
    assert_eq!(peer.mismatch().unwrap()["mismatch"], "unexpected frame");

    let (_directory, peer) = synthetic_peer();
    let (success, frames) = exchange(&peer, &["other"], &[]);
    assert!(!success);
    assert!(frames.is_empty());
    assert_eq!(
        peer.mismatch().unwrap()["mismatch"],
        "launch arguments differ from the recording"
    );

    let (_directory, peer) = synthetic_peer();
    let _ = exchange(&peer, &["serve"], &[]);
    let (success, _) = exchange(&peer, &["serve"], &[]);
    assert!(!success);
    let progress = peer.progress();
    assert!(progress.iter().any(|entry| entry["process"] == 1
        && entry["mismatch"] == "the adapter launched more processes than the recording"));
}

#[test]
#[should_panic(expected = "the adapter closed stdin before sending an expected frame")]
fn assert_replayed_reports_a_launch_that_closed_before_its_recorded_frames() {
    let (_directory, peer) = synthetic_peer();
    let _ = exchange(&peer, &["serve"], &[]);
    peer.assert_replayed();
}

#[test]
#[should_panic(expected = "launch 0 received 0 recorded adapter frames and then stopped")]
fn assert_replayed_reports_a_recorded_launch_that_never_happened() {
    let (_directory, peer) = synthetic_peer();
    peer.assert_replayed();
}

#[test]
fn recording_tees_each_frame_with_its_direction_and_replays_it_back() {
    let directory = tempfile::tempdir().unwrap();
    let transcript = directory.path().join("echo.jsonl");
    // `cat` echoes each adapter frame, standing in for a real CLI.
    let recording_peer = StdioPeer::record(
        "cli",
        &transcript,
        &Recording {
            program: Path::new("/bin/cat"),
            substitutions: &[("{{client}}", "test-client")],
            environment: &[("REPLAY_TEST", "1")],
            redact: &["machine"],
            meta: &json!({"provider":"echo","cli":"cat"}),
        },
    );
    let frame = json!({"id":1,"method":"open","params":{
        "path":format!("{}/notes", recording_peer.cwd()),"client":"test-client",
        "contact":"someone@example.org","machine":{"id":"m-1"}}});

    let (success, frames) = exchange(&recording_peer, &[], std::slice::from_ref(&frame));

    assert!(success);
    assert_eq!(frames, std::slice::from_ref(&frame));
    let scrubbed = json!({"id":1,"method":"open","params":{
        "path":"{{cwd}}/notes","client":"{{client}}","contact":"user@example.com",
        "machine":"redacted"}});
    let recorded: Vec<Value> = std::fs::read_to_string(&transcript)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        recorded,
        [
            json!({"type":"meta","format":FORMAT,"version":1,"provider":"echo","cli":"cat"}),
            json!({"type":"spawn","process":0,"argv":[]}),
            json!({"type":"stdin","process":0,"frame":scrubbed}),
            json!({"type":"stdout","process":0,"frame":scrubbed}),
        ]
    );

    let replay_peer = StdioPeer::replay("cli", &transcript, &[("{{client}}", "test-client")]);
    let mut sent = json!({"id":9,"method":"open","params":{
        "path":format!("{}/notes", replay_peer.cwd()),"client":"test-client",
        "contact":"user@example.com","machine":"redacted"}});
    let (success, frames) = exchange(&replay_peer, &[], std::slice::from_ref(&sent));
    assert!(success);
    // The echoed frame is a request from the CLI, so its id is the recorded one.
    sent["id"] = json!(1);
    assert_eq!(frames, [sent]);
    replay_peer.assert_replayed();
}
