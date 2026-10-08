use super::*;

fn api(version: Version) -> Api {
    Api::new(
        version,
        reqwest::Url::parse("http://127.0.0.1:1234/").unwrap(),
        "private".into(),
        "/tmp".into(),
    )
    .unwrap()
}

#[test]
fn foreground_and_metadata_share_exact_native_model_and_variant_encodings() {
    let mut request = crate::local::opencode::tests::invocation("/tmp".into());
    request.model = "local/test-model".into();
    request.reasoning_effort = Some("high".into());
    request.instructions = Some("Only answer the question".into());
    let spec = crate::ports::agent_session::AgentSessionSpec {
        provider: "opencode".into(),
        cwd: "/tmp".into(),
        config: domain::agent_runtime::StoredAgentConfig {
            model: Some(request.model.clone()),
            thinking_option_id: request.reasoning_effort.clone(),
            ..Default::default()
        },
    };
    for version in [Version::V1, Version::V2] {
        let api = api(version);
        let foreground = api.prompt_parameters(&request);
        let metadata = api.metadata_parameters(&spec, "private", ("local", "test-model"), "source");
        match version {
            Version::V1 => {
                assert_eq!(api.prompt_suffix(), "/prompt_async");
                assert_eq!(
                    foreground["model"],
                    json!({"providerID":"local","modelID":"test-model"})
                );
                assert_eq!(metadata.prompt["model"], foreground["model"]);
                assert_eq!(foreground["variant"], "high");
                assert_eq!(metadata.prompt["variant"], "high");
                assert_eq!(foreground["system"], "Only answer the question");
                assert_eq!(foreground["messageID"], request.input_id);
                assert_eq!(foreground["parts"][0]["text"], request.prompt);
                assert!(foreground.get("tools").is_none());
                assert_eq!(metadata.prompt["tools"]["*"], false);
                assert_eq!(metadata.create, json!({}));
            }
            Version::V2 => {
                assert_eq!(api.prompt_suffix(), "/prompt");
                assert_eq!(
                    metadata.create["model"],
                    json!({"providerID":"local","id":"test-model","variant":"high"})
                );
                assert_eq!(foreground["text"], request.prompt);
                assert_eq!(foreground["metadata"]["aitInputId"], request.input_id);
                assert!(foreground.get("model").is_none());
                assert!(foreground.get("variant").is_none());
                assert!(foreground.get("system").is_none());
            }
        }
    }
}

#[test]
fn permission_events_borrow_only_the_requested_session_in_both_envelopes() {
    for field in ["properties", "data"] {
        let mut event = json!({"type":"permission.asked"});
        event[field] = json!({"id":"perm1", "sessionID":"ses_one"});
        assert_eq!(
            Api::permission_event(&event, "ses_one").unwrap()["id"],
            "perm1"
        );
        assert!(Api::permission_event(&event, "foreign").is_none());
        assert!(Api::permission_event(&json!({"payload":event.clone()}), "ses_one").is_some());
        event["type"] = json!("server.connected");
        assert!(Api::permission_event(&event, "ses_one").is_none());
    }
    assert!(Api::permission_event(&Value::Null, "ses_one").is_none());
}
