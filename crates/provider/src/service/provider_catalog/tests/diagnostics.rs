use super::*;

#[derive(Debug)]
struct UnavailableClient(String);

impl AgentClient for UnavailableClient {
    fn provider(&self) -> &'static str {
        "opencode"
    }

    fn is_available(&self) -> AgentSessionFuture<'_, bool> {
        Box::pin(async { Ok(false) })
    }

    fn create_session<'a>(
        &'a self,
        _spec: &'a AgentSessionSpec,
    ) -> AgentSessionFuture<'a, Box<dyn AgentSession>> {
        Box::pin(async { Err(AgentSessionError::Unavailable) })
    }

    fn resume_session<'a>(
        &'a self,
        _handle: &'a AgentPersistenceHandle,
        _spec: &'a AgentSessionSpec,
        _purpose: AgentResumePurpose,
    ) -> AgentSessionFuture<'a, Box<dyn AgentSession>> {
        Box::pin(async { Err(AgentSessionError::Unavailable) })
    }

    fn diagnostic(&self) -> AgentSessionFuture<'_, String> {
        Box::pin(async { Ok(self.0.clone()) })
    }
}

#[tokio::test]
async fn unavailable_snapshot_exposes_bounded_native_guidance_and_falls_back_for_invalid_text() {
    let guidance = "OpenCode 2.0.20: upgrade to 2.0.25 or newer.";
    for (message, expected) in [
        (guidance.to_owned(), guidance),
        (String::new(), "Provider executable is unavailable"),
        (" \n ".into(), "Provider executable is unavailable"),
        ("invalid\0text".into(), "Provider executable is unavailable"),
        ("x".repeat(4097), "Provider executable is unavailable"),
    ] {
        let entry = discover(&UnavailableClient(message), "/tmp").await;

        assert_eq!(entry.value["status"], "unavailable");
        assert_eq!(entry.value["error"], expected);
        assert_eq!(entry.value["models"], json!([]));
    }
}
