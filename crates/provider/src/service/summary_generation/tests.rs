use std::collections::VecDeque;
use std::sync::Mutex;

use domain::agent_runtime::AgentPersistenceHandle;
use domain::summary::{SummaryKind, SummarySelection};
use serde_json::json;

use super::*;
use crate::ports::agent_session::{
    AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionFuture,
};
use crate::service::summary_generation::test_config::Configuration;

#[derive(Debug)]
struct Client {
    provider: String,
    outputs: Mutex<VecDeque<Result<String, AgentSessionError>>>,
    calls: Arc<Mutex<Vec<(String, String)>>>,
    wait: bool,
}
impl AgentClient for Client {
    fn discover<'a>(
        &'a self,
        _: &'a str,
    ) -> AgentSessionFuture<'a, crate::protocol::provider::Details> {
        Box::pin(async { Ok(crate::protocol::provider::Details::default()) })
    }

    fn summary_model(&self, _: &[Value]) -> Option<SummarySelection> {
        Some(SummarySelection {
            provider: self.provider.clone(),
            model: Some("small".into()),
            thinking_option_id: None,
        })
    }

    fn provider(&self) -> &str {
        &self.provider
    }
    fn is_available(&self) -> AgentSessionFuture<'_, bool> {
        Box::pin(async { Ok(true) })
    }
    fn create_session<'a>(
        &'a self,
        _: &'a AgentSessionSpec,
    ) -> AgentSessionFuture<'a, Box<dyn AgentSession>> {
        Box::pin(async { panic!("metadata must not create a foreground session") })
    }
    fn resume_session<'a>(
        &'a self,
        _: &'a AgentPersistenceHandle,
        _: &'a AgentSessionSpec,
        _: AgentResumePurpose,
    ) -> AgentSessionFuture<'a, Box<dyn AgentSession>> {
        Box::pin(async { panic!("metadata must not resume a foreground session") })
    }
    fn generate_summary<'a>(
        &'a self,
        spec: &'a AgentSessionSpec,
        prompt: &'a str,
        schema: &'a Value,
    ) -> AgentSessionFuture<'a, String> {
        Box::pin(async move {
            assert_eq!(schema["additionalProperties"], false);
            self.calls.lock().unwrap().push((
                spec.config.model.clone().unwrap_or_default(),
                prompt.to_owned(),
            ));
            if self.wait {
                return std::future::pending().await;
            }
            self.outputs
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Err(AgentSessionError::Unavailable))
        })
    }
}
fn request(root: &std::path::Path) -> SummaryRequest {
    SummaryRequest {
        kind: SummaryKind::Title,
        cwd: root.to_string_lossy().into_owned(),
        context: "Fix titles".into(),
        selection: Some(SummarySelection {
            provider: "codex".into(),
            model: Some("current".into()),
            thinking_option_id: None,
        }),
    }
}

#[tokio::test]
async fn repairs_invalid_json_then_falls_back_and_reads_live_configuration() {
    let root = tempfile::tempdir().unwrap();
    let config = Arc::new(Configuration::default());
    config
        .patch(
            &json!({"metadataGeneration":{"providers":[{"provider":"codex","model":"preferred"}]}}),
        )
        .unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let client = Arc::new(Client {
        provider: "codex".into(),
        outputs: Mutex::new(VecDeque::from([
            Ok("invalid".into()),
            Ok("{}".into()),
            Ok("{\"title\":\"\"}".into()),
            Ok("{\"title\":\"Fixed title\"}".into()),
            Ok("{\"title\":\"Next title\"}".into()),
        ])),
        calls: calls.clone(),
        wait: false,
    });
    let service = Generation::new(config.clone(), vec![client]);
    assert_eq!(
        service.generate(request(root.path())).await.unwrap()["title"],
        "Fixed title"
    );
    let recorded = calls.lock().unwrap().clone();
    assert_eq!(
        recorded.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(),
        ["preferred", "preferred", "preferred", "small"]
    );
    assert!(recorded[1].1.contains("previous response was invalid"));
    config
        .patch(
            &json!({"metadataGeneration":{"providers":[{"provider":"codex","model":"patched"}]}}),
        )
        .unwrap();
    service.generate(request(root.path())).await.unwrap();
    assert_eq!(calls.lock().unwrap().last().unwrap().0, "patched");
}

#[tokio::test]
async fn deadlines_shutdown_and_admission_are_bounded() {
    let root = tempfile::tempdir().unwrap();
    let config = Arc::new(Configuration::default());
    let client = Arc::new(Client {
        provider: "codex".into(),
        outputs: Mutex::new(VecDeque::new()),
        calls: Arc::new(Mutex::new(Vec::new())),
        wait: true,
    });
    let mut service = Generation::new(config, vec![client]);
    service.deadline = Duration::from_millis(40);
    assert_eq!(
        service.generate(request(root.path())).await,
        Err(SummaryError::Unavailable)
    );
    let permit = service.queue.acquire_many(32).await.unwrap();
    assert_eq!(
        service.generate(request(root.path())).await,
        Err(SummaryError::Cancelled)
    );
    drop(permit);
    service.shutdown();
    assert_eq!(
        service.generate(request(root.path())).await,
        Err(SummaryError::Cancelled)
    );
}
