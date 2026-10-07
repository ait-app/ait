use super::*;

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use model::summary::{SummaryError, SummaryFuture, SummaryKind, SummaryRequest, SummarySelection};
use serde_json::json;

#[derive(Debug, Default)]
struct Generator {
    requests: Mutex<Vec<SummaryRequest>>,
    cancelled: AtomicBool,
}

impl provider::summary::SummaryGenerator for Generator {
    fn generate(&self, request: SummaryRequest) -> SummaryFuture<'_> {
        Box::pin(async move {
            if self.cancelled.load(Ordering::SeqCst) {
                return Err(SummaryError::Cancelled);
            }
            self.requests.lock().unwrap().push(request);
            Ok(json!({"title":"Shared summary"}))
        })
    }
    fn shutdown(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn summary_adapter_preserves_input_output_and_shared_cancellation() {
    let generator = Arc::new(Generator::default());
    let workspace = summary_source(generator.clone());
    let git = summary_source(generator.clone());
    let request = SummaryRequest {
        kind: SummaryKind::Title,
        cwd: "/repo".into(),
        context: "Fix summary ownership".into(),
        selection: Some(SummarySelection {
            provider: "codex".into(),
            model: Some("chosen".into()),
            thinking_option_id: None,
        }),
    };
    assert_eq!(
        workspace.generate(request.clone()).await.unwrap(),
        json!({"title":"Shared summary"})
    );
    {
        let recorded = generator.requests.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].kind, request.kind);
        assert_eq!(recorded[0].cwd, request.cwd);
        assert_eq!(recorded[0].context, request.context);
        assert_eq!(recorded[0].selection, request.selection);
    }
    workspace.shutdown();
    assert_eq!(git.generate(request).await, Err(SummaryError::Cancelled));
}
