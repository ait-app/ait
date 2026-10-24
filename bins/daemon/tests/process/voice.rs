use super::transport::method_names;

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use axum::{Json, Router, extract::State, routing::post};
use futures_util::SinkExt;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

use super::{
    Command, Process, Stdio, TOKEN, ready, terminate,
    transport::{Socket, connect, receive, request},
};

#[test]
fn speech_worker_rejects_empty_and_malformed_input_without_starting_a_host() {
    use std::io::Write;
    for input in [b"".as_slice(), b"not-json\n".as_slice()] {
        let root = tempfile::tempdir().unwrap();
        let mut worker = Command::new(env!("CARGO_BIN_EXE_daemon"))
            .arg("--speech-worker")
            .current_dir(root.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        worker.stdin.take().unwrap().write_all(input).unwrap();
        let output = worker.wait_with_output().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}

async fn transcription(State(hang): State<Arc<AtomicBool>>) -> Json<Value> {
    Json(json!({"text":if hang.load(Ordering::SeqCst) { "hold" } else { "hello voice" }}))
}

async fn speech(Json(payload): Json<Value>) -> Vec<u8> {
    assert_eq!(payload["input"], "Claude: hello voice");
    assert_eq!(payload["response_format"], "pcm");
    vec![0, 0, 1, 0]
}

fn start_server(
    root: &std::path::Path,
    path: &std::ffi::OsStr,
    endpoint: &str,
) -> (Process, std::path::PathBuf) {
    let log = root.join("server.log");
    let child = Command::new(env!("CARGO_BIN_EXE_daemon"))
        .current_dir(root)
        .arg("--data-dir")
        .arg(root.join("state"))
        .args(["--listen", "127.0.0.1:0"])
        .env("AIT_SERVER_TOKEN", TOKEN)
        .env("PATH", path)
        .env("AIT_SERVER_CLAUDE_BIN", root.join("claude"))
        .env("CLAUDE_CONFIG_DIR", root.join("claude-config"))
        .env("AIT_SPEECH_PROVIDER", "openai")
        .env("AIT_SPEECH_BASE_URL", endpoint)
        .env("AIT_SPEECH_API_KEY", "offline-voice-secret")
        .env_remove("AIT_SPEECH_STT_PROVIDER")
        .env_remove("AIT_SPEECH_TTS_PROVIDER")
        .env_remove("AIT_SERVER_LISTEN")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    (Process(child), log)
}

#[tokio::test]
async fn legacy_voice_calls_speech_and_non_realtime_provider_then_disconnect_interrupts_owned_turn()
{
    let super::native::NativeFixture { root, cwd, path } = super::native::NativeFixture::new();
    std::os::unix::fs::symlink(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/provider/tests/fixtures/claude_code.py"),
        root.path().join("claude"),
    )
    .unwrap();
    let hang = Arc::new(AtomicBool::new(false));
    let app = Router::new()
        .route("/v1/audio/transcriptions", post(transcription))
        .route("/v1/audio/speech", post(speech))
        .with_state(hang.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let http = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let (mut process, log) = start_server(root.path(), &path, &endpoint);
    let address = ready(&mut process, &log).await;
    let (mut control, id) = create_agent(&address, &cwd, "claude").await;
    let mut client = connect(&address, &method_names(voice::connection::METHODS)).await;
    let mode = request(
        &mut client,
        "voice.mode.set.request",
        json!({"enabled":true,"agentId":id}),
    )
    .await;
    assert_eq!(mode["result"]["accepted"], true, "{mode}");
    let audio = json!({"type":"event","method":"voice.audio.chunk","params":{"audio":"AAgACA==","format":"audio/pcm;rate=16000","isLast":true}});
    client
        .send(Message::Text(audio.to_string().into()))
        .await
        .unwrap();
    let mut transcript_seen = false;
    loop {
        let event = receive(&mut client).await;
        match event["method"].as_str() {
            Some("voice.transcription.result") => {
                assert_eq!(event["params"]["text"], "hello voice");
                transcript_seen = true;
            }
            Some("voice.audio.output") => {
                assert!(transcript_seen);
                assert_eq!(event["params"]["audio"], "AAABAA==");
                client.send(Message::Text(json!({"type":"event","method":"voice.audio.played","params":{"id":event["params"]["id"]}}).to_string().into())).await.unwrap();
                break;
            }
            Some("voice.input.state") => {}
            _ => panic!("unexpected voice event: {event}"),
        }
    }
    hang.store(true, Ordering::SeqCst);
    client
        .send(Message::Text(audio.to_string().into()))
        .await
        .unwrap();
    loop {
        if receive(&mut client).await["method"] == "voice.transcription.result" {
            break;
        }
    }
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if request(&mut control, "agent.get.request", json!({"agentId":id})).await["result"]["agent"]["status"] == "running" { break; }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }).await.unwrap();
    client.close(None).await.unwrap();
    let final_state = request(
        &mut control,
        "agent.finish.wait.request",
        json!({"agentId":id,"timeoutMs":5000}),
    )
    .await;
    assert_eq!(final_state["result"]["status"], "idle", "{final_state}");
    terminate(&mut process).await;
    assert!(
        !std::fs::read_to_string(&log)
            .unwrap()
            .contains("offline-voice-secret")
    );
    http.abort();
}

async fn create_agent(address: &str, cwd: &std::path::Path, provider: &str) -> (Socket, String) {
    let mut control = connect(
        address,
        &[
            "workspace.open.request",
            "agent.create.request",
            "agent.get.request",
            "agent.finish.wait.request",
        ],
    )
    .await;
    assert_eq!(
        request(&mut control, "workspace.open.request", json!({"cwd":cwd})).await["type"],
        "response"
    );
    let agent = request(
        &mut control,
        "agent.create.request",
        json!({"config":{"provider":provider,"cwd":cwd}}),
    )
    .await;
    let id = agent["result"]["agentId"].as_str().unwrap().to_owned();
    assert_eq!(
        agent["result"]["agent"]["capabilities"]["supportsRealtimeVoice"] == true,
        provider == "codex",
    );
    (control, id)
}

async fn unexpected_speech(State(calls): State<Arc<AtomicUsize>>) -> Json<Value> {
    calls.fetch_add(1, Ordering::SeqCst);
    Json(json!({"text":"unexpected generic speech request"}))
}

#[tokio::test]
async fn native_voice_routes_audio_without_speech_and_disconnect_stops_the_provider_session() {
    let super::native::NativeFixture { root, cwd, path } = super::native::NativeFixture::new();
    let speech_calls = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route("/v1/audio/transcriptions", post(unexpected_speech))
        .route("/v1/audio/speech", post(unexpected_speech))
        .with_state(speech_calls.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let http = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let (mut process, log) = start_server(root.path(), &path, &endpoint);
    let address = ready(&mut process, &log).await;
    let (_control, id) = create_agent(&address, &cwd, "codex").await;
    let mut client = connect(&address, &method_names(voice::connection::METHODS)).await;
    let mode = request(
        &mut client,
        "voice.mode.set.request",
        json!({"enabled":true,"agentId":id}),
    )
    .await;
    assert_eq!(mode["result"]["accepted"], true, "{mode}");
    client
        .send(Message::Text(
            json!({
                "type":"event","method":"voice.audio.chunk",
                "params":{"audio":"AAgACA==","format":"audio/pcm;rate=24000","isLast":true}
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    let mut transcript_seen = false;
    loop {
        let event = receive(&mut client).await;
        match event["method"].as_str() {
            Some("voice.transcription.result") => {
                assert_eq!(event["params"]["text"], "hello");
                transcript_seen = true;
            }
            Some("voice.audio.output") => {
                assert!(transcript_seen);
                assert_eq!(event["params"]["audio"], "AAgACA==");
                assert_eq!(
                    event["params"]["format"],
                    "audio/pcm;rate=24000;bits=16;channels=1"
                );
                break;
            }
            Some("voice.input.state") => {}
            _ => panic!("unexpected native voice event: {event}"),
        }
    }
    client.close(None).await.unwrap();
    let requests = wait_native_stop(&cwd).await;
    assert!(
        requests
            .iter()
            .any(|request| request["method"] == "thread/realtime/start")
    );
    assert!(
        !requests
            .iter()
            .any(|request| request["method"] == "turn/start")
    );
    assert_eq!(speech_calls.load(Ordering::SeqCst), 0);
    terminate(&mut process).await;
    http.abort();
}

async fn wait_native_stop(cwd: &std::path::Path) -> Vec<Value> {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let requests: Vec<Value> = std::fs::read_to_string(cwd.join("native-requests.jsonl"))
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            if requests
                .iter()
                .any(|request| request["method"] == "thread/realtime/stop")
            {
                return requests;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("disconnect must stop the native realtime session")
}
