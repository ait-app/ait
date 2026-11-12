use super::*;

#[derive(Debug)]
struct UnavailableClient(String);

#[async_trait::async_trait]
impl AgentClient for UnavailableClient {
    fn provider(&self) -> &'static str {
        "opencode"
    }

    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        Ok(false)
    }

    async fn create_session(
        &self,
        _spec: &AgentSessionSpec,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        Err(AgentSessionError::Unavailable)
    }

    async fn resume_session(
        &self,
        _handle: &AgentPersistenceHandle,
        _spec: &AgentSessionSpec,
        _purpose: AgentResumePurpose,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        Err(AgentSessionError::Unavailable)
    }

    async fn diagnostic(&self) -> Result<String, AgentSessionError> {
        Ok(self.0.clone())
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

#[tokio::test]
async fn available_list_exposes_native_unavailability_guidance() {
    let guidance = "OpenCode 2.0.20: upgrade to 2.0.25 or newer.";
    let clients: BTreeMap<String, Arc<dyn AgentClient>> = BTreeMap::from([(
        "opencode".to_owned(),
        Arc::new(UnavailableClient(guidance.to_owned())) as Arc<dyn AgentClient>,
    )]);

    let value = Catalog::default()
        .execute(
            &clients,
            &SessionEvents::default(),
            "provider.available.list.request",
            json!({}),
        )
        .await
        .unwrap();

    assert_eq!(value["providers"][0]["available"], false);
    assert_eq!(value["providers"][0]["error"], guidance);
}
