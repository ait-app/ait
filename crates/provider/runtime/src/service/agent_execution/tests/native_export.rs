use std::io::{Read, Seek, Write};
use std::sync::Mutex;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
use provider_core::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionSpec,
    AgentTurnEvent,
};
use provider_core::ports::native_html_export::{NativeHtmlExport, NativeHtmlExportError};
use provider_core::protocol::prompt::AgentPrompt;

use super::*;

#[derive(Debug, Default)]
struct NativeState {
    admitted: usize,
    cancelled: usize,
    ready: bool,
}

#[derive(Debug)]
struct Client(Arc<Mutex<NativeState>>);

#[async_trait::async_trait]
impl AgentClient for Client {
    fn provider(&self) -> &'static str {
        "omp"
    }
    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        Ok(true)
    }
    async fn create_session(
        &self,
        _: &AgentSessionSpec,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        Ok(Box::new(Session {
            state: self.0.clone(),
            pending: false,
        }))
    }
    async fn resume_session(
        &self,
        _: &AgentPersistenceHandle,
        spec: &AgentSessionSpec,
        _: AgentResumePurpose,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        self.create_session(spec).await
    }
}

#[derive(Debug)]
struct Session {
    state: Arc<Mutex<NativeState>>,
    pending: bool,
}

#[async_trait::async_trait]
impl AgentSession for Session {
    fn provider(&self) -> &'static str {
        "omp"
    }
    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        None
    }
    async fn runtime_info(&mut self) -> Result<StoredAgentRuntimeInfo, AgentSessionError> {
        Ok(StoredAgentRuntimeInfo {
            provider: "omp".to_owned(),
            session_id: None,
            model: None,
            thinking_option_id: None,
            mode_id: None,
            extra: None,
        })
    }
    async fn start_input(
        &mut self,
        _: &AgentPrompt,
        _: &StoredAgentConfig,
    ) -> Result<String, AgentSessionError> {
        Ok("turn".to_owned())
    }
    fn poll_turn(&mut self) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        Ok(None)
    }
    fn begin_native_html_export(&mut self) -> Result<(), AgentSessionError> {
        self.pending = true;
        let mut state = self.state.lock().unwrap();
        state.admitted += 1;
        state.ready = false;
        Ok(())
    }
    fn take_native_html_export(
        &mut self,
    ) -> Option<Result<NativeHtmlExport, NativeHtmlExportError>> {
        if !self.pending || !self.state.lock().unwrap().ready {
            return None;
        }
        self.pending = false;
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"native html").unwrap();
        file.rewind().unwrap();
        Some(Ok(NativeHtmlExport {
            file,
            file_name: "native.html".to_owned(),
            size: 11,
        }))
    }
    fn cancel_native_html_export(&mut self) {
        self.pending = false;
        self.state.lock().unwrap().cancelled += 1;
    }
    async fn close(&mut self) -> Result<(), AgentSessionError> {
        Ok(())
    }
}

async fn wait_for(state: &Arc<Mutex<NativeState>>, condition: impl Fn(&NativeState) -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if condition(&state.lock().unwrap()) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("native export state must advance");
}

#[tokio::test]
async fn native_html_export_keeps_lane_available_and_abandoned_call_is_cancelled() {
    let fixture = Fixture::new();
    let state = Arc::new(Mutex::new(NativeState::default()));
    let (execution, _) = worker_with_client(
        &fixture,
        model::creation::Creations::default(),
        Box::new(Client(state.clone())),
    );
    let created = execution
        .execute(
            "agent.create.request",
            json!({"config":{"provider":"omp","cwd":fixture.cwd}}),
        )
        .await
        .unwrap();
    let id = created["agentId"].as_str().unwrap().to_owned();
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"keep running"}),
        )
        .await
        .unwrap();
    let request = crate::ports::native_html_export::Request {
        agent_id: id.clone(),
    };
    let worker = execution.clone();
    let export = tokio::spawn(async move { worker.export_native_html(request).await });
    wait_for(&state, |state| state.admitted == 1).await;
    let permission = tokio::time::timeout(
        Duration::from_secs(2),
        execution.execute(
            "agent.permission.resolve.request",
            json!({"agentId":id, "requestId":"missing", "response":{"behavior":"deny"}}),
        ),
    )
    .await
    .expect("export must not block the session lane");
    assert!(permission.is_err());
    state.lock().unwrap().ready = true;
    let mut artifact = tokio::time::timeout(Duration::from_secs(5), export)
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .artifact;
    let mut actual = String::new();
    artifact.file.read_to_string(&mut actual).unwrap();
    assert_eq!(actual, "native html");
    assert_eq!(artifact.agent_id, id);
    let worker = execution.clone();
    let request = crate::ports::native_html_export::Request {
        agent_id: id.clone(),
    };
    let abandoned = tokio::spawn(async move { worker.export_native_html(request).await });
    wait_for(&state, |state| state.admitted == 2).await;
    abandoned.abort();
    wait_for(&state, |state| state.cancelled == 1).await;
    execution.shutdown().await.unwrap();
}
