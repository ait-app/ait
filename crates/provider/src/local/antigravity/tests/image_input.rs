use super::*;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

use crate::protocol::timeline::FetchRequest;
use crate::rpc::timeline::fetch_stored;
use crate::service::agent_manager::{AgentManager, AgentRegistration};
use crate::storage::timeline::Timeline;

#[tokio::test]
async fn image_requests_reach_agy_and_remain_visible_in_reopened_history() {
    let (directory, client, spec) = fixture();
    let database = directory.path().join("timeline.sqlite3");
    let timeline = Timeline::open(&database).unwrap();
    let registry = FileBackedAgentRuntimeRegistry::new(directory.path().join("agents.json"));
    let mut manager = AgentManager::new(Box::new(registry)).with_timeline(timeline.clone());
    let environment = client.environment.clone();
    manager.register_client(Box::new(client)).unwrap();
    manager
        .create_with_environment("agent", &spec, AgentRegistration::default(), &environment)
        .await
        .unwrap();
    let image = PromptImage {
        data: "aGVsbG8=".to_owned(),
        mime_type: "image/png".to_owned(),
    };
    for (text, id) in [("move refresh button", "screenshot"), ("", "image-only")] {
        let prompt = AgentPrompt {
            text: text.to_owned(),
            images: vec![image.clone()],
            client_message_id: Some(id.to_owned()),
            ..Default::default()
        };
        manager.send_input("agent", &prompt).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while manager.active_turn("agent").is_some() {
                manager.poll().await.unwrap();
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }
    manager.close_all().await.unwrap();
    drop(manager);
    drop(timeline);
    let timeline = Timeline::open(&database).unwrap();
    let request: FetchRequest =
        serde_json::from_value(json!({"agentId":"agent","direction":"tail","limit":30})).unwrap();
    let page = fetch_stored(&request, &timeline, "agent", &Value::Null, Some(false)).unwrap();
    let messages: Vec<_> = page["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| {
            matches!(
                entry["item"]["type"].as_str(),
                Some("user_message" | "assistant_message")
            )
        })
        .collect();
    assert_eq!(messages.len(), 4);
    let mut sources = Vec::new();
    for (pair, (text, id)) in messages
        .as_chunks::<2>()
        .0
        .iter()
        .zip([("move refresh button", "screenshot"), ("", "image-only")])
    {
        assert_eq!(pair[0]["item"]["type"], "user_message");
        assert_eq!(pair[0]["item"]["text"], text);
        assert_eq!(pair[0]["item"]["clientMessageId"], id);
        assert_eq!(pair[1]["item"]["type"], "assistant_message");
        assert!(pair[0]["seqEnd"].as_u64().unwrap() < pair[1]["seqStart"].as_u64().unwrap());
        let images = pair[0]["item"]["imageSources"].as_array().unwrap();
        assert_eq!(images.len(), 1);
        let source = images[0].as_str().unwrap();
        let path = Path::new(source);
        assert!(path.is_absolute());
        assert_eq!(path.extension().unwrap(), "png");
        assert_eq!(std::fs::read(path).unwrap(), b"hello");
        assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(
            path.parent()
                .unwrap()
                .metadata()
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        sources.push(source);
    }
    assert_eq!(sources[0], sources[1]);
    let calls = logs(&directory);
    let sent: Vec<_> = calls.iter().filter_map(|call| call.get("input")).collect();
    assert_eq!(sent.len(), 2);
    for (input, source) in sent.into_iter().zip(sources) {
        let blocks = input["message"]["content"].as_array().unwrap();
        assert!(blocks.iter().all(|block| block["type"] == "text"));
        assert_eq!(
            blocks.last().unwrap()["text"],
            format!("Attached image: {source}")
        );
        assert!(!input.to_string().contains(&image.data));
    }
}

#[tokio::test]
async fn unsupported_schema_has_an_actionable_reason_before_admission() {
    let (directory, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let prompt = AgentPrompt {
        text: "schema".to_owned(),
        output_schema: Some(json!({})),
        ..Default::default()
    };
    assert_eq!(
        session.start_input(&prompt, &spec.config).await,
        Err(AgentSessionError::Rejected)
    );
    assert!(
        session
            .failure_message()
            .is_some_and(|message| message.contains("JSON schema"))
    );
    assert_eq!(session.failure_diagnostic().unwrap().source, "input");
    assert_eq!(session.poll_turn().unwrap(), None);
    assert!(
        logs(&directory)
            .iter()
            .all(|call| call.get("input").is_none())
    );
    session
        .start_turn("valid retry", &spec.config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    assert!(session.failure_message().is_none());
    session.close().await.unwrap();
}

#[tokio::test]
async fn multiple_images_keep_attachment_order_without_exposing_paths_as_user_text() {
    let (directory, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let prompt = AgentPrompt {
        text: "question".to_owned(),
        images: ["image/png", "image/jpeg", "image/gif", "image/webp"]
            .into_iter()
            .map(|mime| PromptImage {
                data: "aGVsbG8=".to_owned(),
                mime_type: mime.to_owned(),
            })
            .collect(),
        attachments: vec![
            json!({"type":"text","mimeType":"text/plain","text":"notes"}),
            json!({"type":"text","mimeType":"text/plain","text":"context","contextKind":"chat_history"}),
        ],
        ..Default::default()
    };
    session.start_input(&prompt, &spec.config).await.unwrap();
    let events = completed(session.as_mut()).await;
    let input = events
        .iter()
        .find_map(|event| match event {
            AgentTurnEvent::Timeline(entry) if entry.item["type"] == "user_message" => {
                Some(&entry.item)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(input["text"], "context\nquestion\nnotes");
    let sources = input["imageSources"].as_array().unwrap();
    assert_eq!(sources.len(), 4);
    let calls = logs(&directory);
    let sent = calls.iter().find_map(|call| call.get("input")).unwrap();
    let blocks = sent["message"]["content"].as_array().unwrap();
    assert_eq!(blocks.len(), 7);
    assert_eq!(blocks[0]["text"], "context");
    assert_eq!(blocks[1]["text"], "question");
    assert_eq!(blocks[6]["text"], "notes");
    for ((block, source), extension) in blocks[2..6]
        .iter()
        .zip(sources)
        .zip(["png", "jpg", "gif", "webp"])
    {
        let source = source.as_str().unwrap();
        assert_eq!(block["text"], format!("Attached image: {source}"));
        assert_eq!(Path::new(source).extension().unwrap(), extension);
        assert_eq!(std::fs::read(source).unwrap(), b"hello");
    }
    session.close().await.unwrap();
}

#[tokio::test]
async fn invalid_images_and_storage_failure_are_reported_before_native_admission() {
    let (directory, client, spec) = fixture();
    let image_directory = directory.path().canonicalize().unwrap().join("images");
    let mut session = client.create_session(&spec).await.unwrap();
    let mut prompt = AgentPrompt {
        images: vec![
            PromptImage {
                data: "aGVsbG8=".to_owned(),
                mime_type: "image/png".to_owned(),
            },
            PromptImage {
                data: "invalid base64".to_owned(),
                mime_type: "image/png".to_owned(),
            },
        ],
        ..Default::default()
    };
    assert_eq!(
        session.start_input(&prompt, &spec.config).await,
        Err(AgentSessionError::Rejected)
    );
    assert!(
        session
            .failure_message()
            .unwrap()
            .contains("invalid or too large")
    );
    assert!(
        !image_directory.exists(),
        "validate every image before creating artifacts"
    );
    prompt.images.pop();
    std::fs::write(&image_directory, b"not a directory").unwrap();
    assert_eq!(
        session.start_input(&prompt, &spec.config).await,
        Err(AgentSessionError::Failed)
    );
    assert!(
        session
            .failure_message()
            .unwrap()
            .contains("could not save AGY image attachments")
    );
    assert_eq!(session.failure_diagnostic().unwrap().source, "input");
    assert_eq!(session.poll_turn().unwrap(), None);
    assert!(
        logs(&directory)
            .iter()
            .all(|call| call.get("input").is_none())
    );
    std::fs::remove_file(image_directory).unwrap();
    session.start_input(&prompt, &spec.config).await.unwrap();
    completed(session.as_mut()).await;
    assert!(session.failure_message().is_none());
    session.close().await.unwrap();
}

#[tokio::test]
async fn invalid_configuration_keeps_a_specific_input_diagnostic() {
    let (directory, client, mut spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    spec.config.system_prompt = Some("unsupported".to_owned());
    assert_eq!(
        session.start_turn("test", &spec.config).await,
        Err(AgentSessionError::Rejected)
    );
    assert!(
        session
            .failure_message()
            .unwrap()
            .contains("session configuration")
    );
    assert_eq!(session.failure_diagnostic().unwrap().source, "input");
    assert!(
        logs(&directory)
            .iter()
            .all(|call| call.get("input").is_none())
    );
    session.close().await.unwrap();
}
