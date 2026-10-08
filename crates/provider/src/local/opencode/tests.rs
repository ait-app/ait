use super::*;
use crate::local::opencode::types::{Content, Role};
use crate::local::opencode::types::{DenyApprovals, Outcome, ProgressEvent};
use async_trait::async_trait;
use serde_json::json;

pub(super) mod fixture;
#[cfg(unix)]
mod installed;

#[derive(Default)]
struct Progress(std::sync::Mutex<Vec<ProgressEvent>>);
#[async_trait]
impl ProgressSink for Progress {
    async fn report(&self, event: ProgressEvent) {
        self.0.lock().unwrap().push(event);
    }
}

pub(super) fn invocation(cwd: PathBuf) -> Invocation {
    Invocation {
        driver: "opencode".into(),
        request_id: "run-1".into(),
        session_id: None,
        input_id: "input-1".into(),
        prompt: "hello".into(),
        instructions: None,
        cwd,
        model: "local/test-model".into(),
        reasoning_effort: None,
        full_access: true,
        verify_settings: true,
        agent: "build".into(),
        approvals: Arc::new(DenyApprovals),
        cancellation: tokio_util::sync::CancellationToken::new(),
        cancel_acknowledged: Arc::default(),
    }
}

#[test]
fn selects_protocol_and_rejects_unsupported_versions() {
    for value in ["1.14.46", "opencode v1.14.46\n"] {
        assert_eq!(
            protocol::Version::parse(value).unwrap(),
            protocol::Version::V1
        );
    }
    for value in ["2.0.10", "v2.1.0-beta.1"] {
        assert_eq!(
            protocol::Version::parse(value).unwrap(),
            protocol::Version::V2
        );
    }
    for value in ["2.0.9", "3.0.0", "garbage", "1.1", "0.1.0"] {
        assert!(protocol::Version::parse(value).is_err());
    }
}

#[test]
fn rejects_remote_loopback_lookalikes_and_credentials() {
    for base in [
        "http://localhost:1234/",
        "https://127.0.0.1:1234/",
        "http://127.0.0.1:1234/path",
        "http://user@127.0.0.1:1234/",
        "http://127.0.0.1:1234/?token=x",
        "http://127.0.0.1/",
    ] {
        assert!(
            http::Api::new(
                protocol::Version::V1,
                reqwest::Url::parse(base).unwrap(),
                "private".into(),
                "/tmp".into()
            )
            .is_err()
        );
    }
}

#[test]
fn refuses_sandbox_emulation_resume_instructions_and_unsafe_session_ids() {
    let mut request = invocation("/tmp".into());
    {
        request.full_access = false;
        assert_eq!(
            session::validate(&request).unwrap_err().code,
            Fault::AgentCapabilityUnsupported
        );
    }
    request.full_access = true;
    request.session_id = Some("../other".into());
    assert!(session::validate(&request).is_err());
    request.session_id = Some("ses_one".into());
    request.instructions = Some("replace".into());
    assert!(session::validate(&request).is_err());
    request.instructions = None;
    request.cancellation.cancel();
    assert_eq!(
        session::validate(&request).unwrap_err().code,
        Fault::RunCancelled
    );
}

#[test]
fn maps_terminal_native_tools_to_native_records_and_redacts_secret_fields() {
    let records = json!([
        {"id":"u1","type":"user","text":"hello","metadata":{"aitInputId":"input-1"},"time":{"created":1}},
        {"id":"a1","type":"assistant","time":{"created":2,"completed":3},"content":[
            {"type":"text","text":"working"},
            {"id":"call1","type":"tool","name":"shell","state":{"status":"completed","input":{"command":"pwd","token":"secret"},"content":[{"type":"text","text":"/tmp"}]}},
            {"type":"reasoning","text":"thought","providerState":{"token":"secret"}}
        ]}
    ]);
    let mapped = history::normalize(
        protocol::Version::V2,
        "ses_one",
        records.as_array().unwrap(),
    )
    .unwrap();
    assert_eq!(mapped.len(), 3);
    assert_eq!(mapped[0].input_id.as_deref(), Some("input-1"));
    assert!(
        matches!(&mapped[1].sub_messages[1],Content::ToolCall(tool) if tool.call_id=="call1" && !tool.arguments.contains("secret"))
    );
    assert_eq!(mapped[2].role, Role::User);
    assert!(mapped[2].tool_result.is_some());
    assert!(!serde_json::to_string(&mapped).unwrap().contains("secret"));
}

#[test]
fn rejects_duplicate_and_unfinished_history() {
    let assistant = json!({"id":"a1","type":"assistant","time":{"created":2},"content":[]});
    assert!(
        history::normalize(
            protocol::Version::V2,
            "ses_one",
            std::slice::from_ref(&assistant)
        )
        .is_err()
    );
    let mut complete = assistant;
    complete["time"]["completed"] = json!(3);
    assert!(
        history::normalize(
            protocol::Version::V2,
            "ses_one",
            &[complete.clone(), complete]
        )
        .is_err()
    );
    let wrong = json!({"info":{"id":"u1","role":"user","sessionID":"other","time":{"created":1}},"parts":[]});
    assert!(history::normalize(protocol::Version::V1, "ses_one", &[wrong]).is_err());
    let running = json!({"id":"a1","type":"assistant","time":{"created":1,"completed":2},"content":[{"type":"tool","id":"c","name":"shell","state":{"status":"running","input":{}}}]});
    assert!(history::normalize(protocol::Version::V2, "ses_one", &[running]).is_err());
}

#[tokio::test]
async fn native_v1_and_v2_prepare_send_once_read_and_resume() {
    for version in [protocol::Version::V1, protocol::Version::V2] {
        let fixture = fixture::Fixture::start(version).await;
        let adapter = Driver::new(fixture.binary.clone());
        let request = invocation(fixture.cwd.clone());
        let mut connection = adapter.open(request.clone()).await.unwrap();
        assert_eq!(fixture.state.lock().unwrap().submissions, 0);
        assert!(connection.prepared().messages.is_empty());
        if version == protocol::Version::V1 {
            assert!(connection.prepared().input_id.starts_with("msg_"));
        }
        let snapshot = connection
            .start(Arc::new(Progress::default()))
            .await
            .unwrap();
        assert_eq!(snapshot.outcome, Some(Outcome::Completed));
        assert_eq!(snapshot.messages.len(), 2);
        assert_eq!(fixture.state.lock().unwrap().submissions, 1);
        assert!(
            connection
                .start(Arc::new(Progress::default()))
                .await
                .is_err()
        );
        assert_eq!(connection.read().await.unwrap().messages, snapshot.messages);
        connection.close().await;
        let mut resumed = request;
        resumed.session_id = Some(snapshot.id);
        resumed.input_id = snapshot.input_id;
        let mut connection = adapter.open(resumed).await.unwrap();
        assert_eq!(connection.prepared().messages, snapshot.messages);
        connection.close().await;
        assert_eq!(fixture.state.lock().unwrap().submissions, 1);
    }
}

#[tokio::test]
async fn model_catalog_uses_connected_native_models_and_variants() {
    for version in [protocol::Version::V1, protocol::Version::V2] {
        let fixture = fixture::Fixture::start(version).await;
        let models = Driver::new(fixture.binary.clone())
            .discover_models(fixture.cwd.clone())
            .await
            .unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "local/test-model");
        assert_eq!(models[0].reasoning_efforts, vec!["high"]);
    }
}

#[tokio::test]
async fn v2_partial_nonempty_catalog_waits_for_initial_plugin_activation() {
    let fixture = fixture::Fixture::start(protocol::Version::V2).await;
    fixture.state.lock().unwrap().pending_plugin_polls = 3;
    let driver = Driver::new(fixture.binary.clone());
    let models = driver.discover_models(fixture.cwd.clone()).await.unwrap();
    assert_eq!(
        models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        vec!["local/test-model"]
    );
    assert_eq!(fixture.state.lock().unwrap().pending_plugin_polls, 0);
    assert_eq!(fixture.state.lock().unwrap().submissions, 0);
}

#[tokio::test]
async fn v2_cold_model_catalog_retries_are_bounded() {
    let fixture = fixture::Fixture::start(protocol::Version::V2).await;
    fixture.state.lock().unwrap().empty_model_catalogs = 2;
    let driver = Driver::new(fixture.binary.clone());
    let models = driver.discover_models(fixture.cwd.clone()).await.unwrap();
    assert_eq!(models[0].id, "local/test-model");
    assert_eq!(fixture.state.lock().unwrap().empty_model_catalogs, 0);

    for pending_plugins in [false, true] {
        {
            let mut state = fixture.state.lock().unwrap();
            state.empty_model_catalogs = if pending_plugins { 0 } else { usize::MAX };
            state.pending_plugin_polls = if pending_plugins { usize::MAX } else { 0 };
        }
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(8),
            driver.discover_models(fixture.cwd.clone()),
        )
        .await
        .expect("an unready native catalog must not hang or return partial models");
        assert_eq!(result.unwrap_err().code, Fault::ProviderFailed);
    }
    assert_eq!(fixture.state.lock().unwrap().submissions, 0);
}

#[tokio::test]
async fn v2_synced_log_and_idle_history_complete_without_replaying_input() {
    for expected in [Outcome::Completed, Outcome::Failed, Outcome::Interrupted] {
        let fixture = fixture::Fixture::start(protocol::Version::V2).await;
        {
            let mut state = fixture.state.lock().unwrap();
            state.idle_completion = expected != Outcome::Interrupted;
            state.aborted_completion = expected == Outcome::Interrupted;
            state.early_failure = expected == Outcome::Failed;
        }
        let driver = Driver::new(fixture.binary.clone());
        let mut request = invocation(fixture.cwd.clone());
        let mut connection = driver.open(request.clone()).await.unwrap();
        let snapshot = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            connection.start(Arc::new(Progress::default())),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(snapshot.outcome, Some(expected));
        connection.close().await;
        request.session_id = Some(snapshot.id);
        let mut resumed = driver.open(request).await.unwrap();
        assert_eq!(resumed.prepared().messages, snapshot.messages);
        assert_eq!(fixture.state.lock().unwrap().submissions, 1);
        resumed.close().await;
    }
}

#[tokio::test]
async fn response_ambiguity_reconciles_without_replaying_input() {
    let fixture = fixture::Fixture::start(protocol::Version::V2).await;
    fixture.state.lock().unwrap().reject_ack = true;
    let mut connection = Driver::new(fixture.binary.clone())
        .open(invocation(fixture.cwd.clone()))
        .await
        .unwrap();
    let history = connection
        .start(Arc::new(Progress::default()))
        .await
        .unwrap();
    assert_eq!(history.outcome, Some(Outcome::Completed));
    assert_eq!(fixture.state.lock().unwrap().submissions, 1);
    connection.close().await;
}

#[tokio::test]
async fn active_session_and_unknown_model_fail_before_input() {
    let fixture = fixture::Fixture::start(protocol::Version::V2).await;
    let adapter = Driver::new(fixture.binary.clone());
    let mut request = invocation(fixture.cwd.clone());
    request.model = "local/missing".into();
    assert!(adapter.open(request).await.is_err());
    let mut connection = adapter.open(invocation(fixture.cwd.clone())).await.unwrap();
    let id = connection.prepared().id.clone();
    connection.close().await;
    fixture.state.lock().unwrap().busy = true;
    let mut request = invocation(fixture.cwd.clone());
    request.session_id = Some(id);
    assert_eq!(
        adapter.open(request).await.err().unwrap().code,
        Fault::SessionBusy
    );
    assert_eq!(fixture.state.lock().unwrap().submissions, 0);
}

#[tokio::test]
async fn history_cursor_cycles_fail_closed() {
    let fixture = fixture::Fixture::start(protocol::Version::V2).await;
    fixture.state.lock().unwrap().cursor_cycle = true;
    let result = Driver::new(fixture.binary.clone())
        .open(invocation(fixture.cwd.clone()))
        .await;
    assert_eq!(result.err().unwrap().code, Fault::ProviderFailed);
    assert_eq!(fixture.state.lock().unwrap().submissions, 0);
}

#[tokio::test]
async fn cancelling_an_active_execution_interrupts_without_replaying() {
    let fixture = fixture::Fixture::start(protocol::Version::V2).await;
    let request = invocation(fixture.cwd.clone());
    let cancellation = request.cancellation.clone();
    let mut connection = Driver::new(fixture.binary.clone())
        .open(request)
        .await
        .unwrap();
    fixture.state.lock().unwrap().busy = true;
    let task = tokio::spawn(async move {
        let result = connection.start(Arc::new(Progress::default())).await;
        connection.close().await;
        result
    });
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while fixture.state.lock().unwrap().submissions == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    cancellation.cancel();
    let result = tokio::time::timeout(std::time::Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.unwrap_err().code, Fault::RunCancelled);
    let state = fixture.state.lock().unwrap();
    assert_eq!(state.submissions, 1);
    assert!(!state.busy);
}

#[tokio::test]
async fn native_failure_without_assistant_content_is_terminal() {
    let fixture = fixture::Fixture::start(protocol::Version::V2).await;
    fixture.state.lock().unwrap().early_failure = true;
    let mut connection = Driver::new(fixture.binary.clone())
        .open(invocation(fixture.cwd.clone()))
        .await
        .unwrap();
    let history = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        connection.start(Arc::new(Progress::default())),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(history.outcome, Some(Outcome::Failed));
    assert_eq!(history.messages.len(), 1);
    connection.close().await;
}

#[tokio::test]
async fn native_budget_overrun_interrupts_execution() {
    let fixture = fixture::Fixture::start(protocol::Version::V2).await;
    let adapter = Driver::new(fixture.binary.clone())
        .with_execution_limits(OpenCodeExecutionLimits {
            max_output_bytes: 1,
            ..Default::default()
        })
        .unwrap();
    let mut connection = adapter.open(invocation(fixture.cwd.clone())).await.unwrap();
    let result = connection.start(Arc::new(Progress::default())).await;
    assert_eq!(result.unwrap_err().code, Fault::RunLimitExceeded);
    assert_eq!(fixture.state.lock().unwrap().submissions, 1);
    connection.close().await;
}
