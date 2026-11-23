use super::*;

/// A ready adapter whose catalog presentation is chosen by the test.
#[derive(Debug)]
struct Presented {
    presentation: Option<CatalogPresentation>,
    modes: Vec<Value>,
}

#[async_trait::async_trait]
impl AgentClient for Presented {
    fn provider(&self) -> &'static str {
        "presented"
    }
    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        Ok(true)
    }
    async fn discover(&self, _cwd: &str) -> Result<Details, AgentSessionError> {
        Ok(Details {
            models: Vec::new(),
            modes: self.modes.clone(),
            features: Vec::new(),
        })
    }
    fn catalog_presentation(&self) -> Option<CatalogPresentation> {
        self.presentation
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
}

fn presented(default_mode: DefaultMode, modes: Vec<Value>) -> Presented {
    Presented {
        presentation: Some(CatalogPresentation {
            label: "Presented",
            description: "Presented via a fixture",
            default_mode,
        }),
        modes,
    }
}

#[tokio::test]
async fn adapter_presentation_sets_label_description_and_default_mode() {
    let modes = vec![
        json!({"id":"plan","label":"Plan"}),
        json!({"id":"ask","label":"Ask","isDefault":true}),
    ];
    let unflagged = vec![json!({"id":"plan","label":"Plan"})];
    for (client, expected) in [
        (presented(DefaultMode::Omitted, modes.clone()), None),
        (
            presented(DefaultMode::Fixed("full"), modes.clone()),
            Some(json!("full")),
        ),
        (
            presented(DefaultMode::Discovered, modes),
            Some(json!("ask")),
        ),
        (
            presented(DefaultMode::Discovered, unflagged),
            Some(Value::Null),
        ),
    ] {
        let entry = discover(&client, "/").await.value;

        assert_eq!(entry["label"], "Presented");
        assert_eq!(entry["description"], "Presented via a fixture");
        assert_eq!(entry.get("defaultModeId"), expected.as_ref());
    }
}

#[tokio::test]
async fn adapters_without_presentation_publish_only_discovered_fields() {
    let client = Presented {
        presentation: None,
        modes: vec![json!({"id":"ask","isDefault":true})],
    };

    let entry = discover(&client, "/").await.value;

    for field in ["label", "description", "defaultModeId"] {
        assert!(entry.get(field).is_none(), "{field}");
    }
}
