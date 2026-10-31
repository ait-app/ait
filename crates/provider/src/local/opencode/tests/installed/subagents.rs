use super::*;
use crate::ports::controls::SubagentEvent;

pub(super) fn model_response(request: &Value) -> Option<Response> {
    if is_user_message(request, "Native child sentinel")
        || is_user_message(
            request,
            "You are a subagent spawned by another session.\nNative child sentinel",
        )
    {
        return Some(streamed_response(
            &json!({"content":"CHILD_SENTINEL"}),
            false,
        ));
    }
    if !is_user_message(request, "Run native child") {
        return None;
    }
    let has_result = request["messages"]
        .as_array()
        .is_some_and(|messages| messages.iter().any(|message| message["role"] == "tool"));
    if has_result {
        return Some(streamed_response(&json!({"content":"ROOT_DONE"}), false));
    }
    let tools = request["tools"].as_array()?;
    let tool = tools
        .iter()
        .find(|tool| matches!(tool["function"]["name"].as_str(), Some("subagent" | "task")))?;
    let name = tool["function"]["name"].as_str().unwrap();
    let agent_key = if name == "task" {
        "subagent_type"
    } else {
        "agent"
    };
    let mut arguments =
        json!({"description":"Explore native child", "prompt":"Native child sentinel"});
    arguments[agent_key] = json!("explore");
    Some(streamed_response(
        &json!({"tool_calls":[{"index":0,"id":"call_child","type":"function",
        "function":{"name":name,"arguments":arguments.to_string()}}]}),
        true,
    ))
}

#[tokio::test]
#[ignore = "requires AIT_TEST_OPENCODE_BIN; isolated XDG and loopback model"]
async fn installed_acp_subagents_stream_and_restore_native_children() {
    let mut fixture = installed(false).await;
    fixture.spec.config.feature_values =
        Some(BTreeMap::from([("permission".into(), json!("allow"))]));
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn("Run native child", &fixture.spec.config)
        .await
        .unwrap();
    let mut saw_running = false;
    let mut saw_text = false;
    let mut child_output = Vec::new();
    loop {
        match event(session.as_mut()).await {
            AgentTurnEvent::Subagent(SubagentEvent::Upsert(child)) => {
                saw_running |= child.descriptor["status"] == "running";
            }
            AgentTurnEvent::Subagent(SubagentEvent::Progress { entry, .. }) => {
                child_output.push(entry.item.clone());
                saw_text |= entry.item["text"]
                    .as_str()
                    .is_some_and(|text| text.contains("CHILD_SENTINEL"));
            }
            AgentTurnEvent::Completed(text) => {
                assert_eq!(text.as_deref(), Some("ROOT_DONE"));
                break;
            }
            AgentTurnEvent::Failed | AgentTurnEvent::Cancelled => {
                panic!("native parent failed: {:?}", session.failure_message())
            }
            _ => {}
        }
    }
    assert!(saw_running, "native child creation was not surfaced");
    assert!(
        saw_text,
        "native child output was not streamed: {child_output:?}"
    );
    let children = session.subagents();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].descriptor["status"], "completed");
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let discovered = fixture.client.subagents(&fixture.spec.cwd).await.unwrap();
    assert_eq!(discovered.len(), 1);
    let child = &discovered[0];
    let history = fixture
        .client
        .inspect_session(child.persistence.as_ref().unwrap(), &child.cwd)
        .await
        .unwrap();
    assert_eq!(
        history.parent_id.as_deref(),
        Some(handle.session_id.as_str())
    );
    assert!(
        history
            .entries
            .iter()
            .any(|entry| entry.item["text"] == "CHILD_SENTINEL")
    );
    let mut restored = fixture
        .client
        .resume_session(&handle, &fixture.spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert!(restored.subagents().is_empty());
    assert_eq!(
        fixture
            .client
            .subagents(&fixture.spec.cwd)
            .await
            .unwrap()
            .len(),
        1
    );
    restored
        .start_turn("Continue", &fixture.spec.config)
        .await
        .unwrap();
    completed(restored.as_mut()).await;
    restored.close().await.unwrap();
}
