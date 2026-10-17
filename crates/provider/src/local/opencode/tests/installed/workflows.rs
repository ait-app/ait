//! Native workflow acceptance; every model request stays on an isolated loopback server.
use super::*;
use crate::protocol::prompt::{AgentPrompt, PromptImage};

fn configuration(fixture: &Installed) -> Value {
    serde_json::from_slice(&std::fs::read(fixture.root.path().join("opencode.json")).unwrap())
        .unwrap()
}

fn write_configuration(directory: &std::path::Path, config: &Value) {
    std::fs::write(directory.join("opencode.json"), config.to_string()).unwrap();
}

async fn request(fixture: &mut Installed, text: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let request = fixture.requests.recv().await.expect("native model request");
            if is_user_message(&request, text) {
                return request;
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "requires OpenCode 2.0.25+; isolated native model, effort and mode selection"]
async fn installed_acp_model_effort_mode_switch_survives_reconnection_and_resume() {
    let mut fixture = installed(false).await;
    let mut config = configuration(&fixture);
    config["providers"]["local"]["models"]["test-model"]["variants"] = json!([]);
    config["providers"]["local"]["models"]["reason-model"] = json!({
        "name":"Reasoning", "limit":{"context":32000,"output":2000},
        "variants":[{"id":"brief","body":{"reasoning_effort":"low"}},
            {"id":"deep","body":{"reasoning_effort":"high"}}]
    });
    write_configuration(fixture.root.path(), &config);
    let details = fixture.client.discover(&fixture.spec.cwd).await.unwrap();
    let plain = details
        .models
        .iter()
        .find(|model| model["id"] == "local/test-model")
        .unwrap();
    assert_eq!(plain["thinkingOptions"], json!([]));
    let reasoning = details
        .models
        .iter()
        .find(|model| model["id"] == "local/reason-model")
        .unwrap();
    assert_eq!(
        reasoning["thinkingOptions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|option| option["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["brief", "deep", "default"]
    );
    fixture.spec.config.model = Some("local/reason-model".into());
    fixture.spec.config.thinking_option_id = Some("deep".into());
    fixture.spec.config.mode_id = Some("plan".into());
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    let identity = session.persistence().unwrap().session_id;
    session
        .start_turn("reason first", &fixture.spec.config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    let first = request(&mut fixture, "reason first").await;
    assert_eq!(first["model"], "reason-model");
    assert_eq!(first["reasoning_effort"], "high");
    let info = session.runtime_info().await.unwrap();
    assert_eq!(info.thinking_option_id.as_deref(), Some("deep"));
    assert_eq!(info.mode_id.as_deref(), Some("plan"));

    fixture.spec.config.model = Some("local/test-model".into());
    fixture.spec.config.thinking_option_id = None;
    fixture.spec.config.mode_id = Some("build".into());
    fixture.spec.config.system_prompt = Some("Switched native system sentinel".into());
    fixture.spec.config.feature_values =
        Some(BTreeMap::from([("permission".into(), json!("deny"))]));
    session
        .start_turn("plain next", &fixture.spec.config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    let second = request(&mut fixture, "plain next").await;
    assert_eq!(second["model"], "test-model");
    assert!(second.get("reasoning_effort").is_none());
    assert!(second["messages"].to_string().contains("reason first"));
    assert!(
        second["messages"]
            .to_string()
            .contains("Switched native system sentinel")
    );
    let info = session.runtime_info().await.unwrap();
    assert_eq!(info.model.as_deref(), Some("local/test-model"));
    assert_eq!(info.thinking_option_id, None);
    assert_eq!(info.mode_id.as_deref(), Some("build"));
    let handle = session.persistence().unwrap();
    assert_eq!(handle.session_id, identity);
    session.close().await.unwrap();

    fixture.spec.config.model = Some("local/reason-model".into());
    fixture.spec.config.thinking_option_id = Some("brief".into());
    let mut resumed = fixture
        .client
        .resume_session(&handle, &fixture.spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    resumed
        .start_turn("reason resumed", &fixture.spec.config)
        .await
        .unwrap();
    completed(resumed.as_mut()).await;
    let third = request(&mut fixture, "reason resumed").await;
    assert_eq!(third["model"], "reason-model");
    assert_eq!(third["reasoning_effort"], "low");
    assert!(third["messages"].to_string().contains("plain next"));
    assert_eq!(resumed.persistence().unwrap().session_id, identity);
    resumed.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires OpenCode 2.0.25+; native large catalog with loopback-only execution"]
async fn installed_acp_large_catalog_is_complete_prompt_free_and_selectable() {
    let mut fixture = installed(false).await;
    let mut config = configuration(&fixture);
    for index in 0..64 {
        config["providers"]["local"]["models"][format!("catalog-{index:02}")] = json!({
            "name":format!("Catalog {index:02}"), "variants":[],
            "limit":{"context":32000,"output":2000}
        });
    }
    write_configuration(fixture.root.path(), &config);
    let models = fixture.client.discover(&fixture.spec.cwd).await.unwrap();
    for index in 0..64 {
        let id = format!("local/catalog-{index:02}");
        assert_eq!(
            models
                .models
                .iter()
                .filter(|model| model["id"] == id)
                .count(),
            1,
            "missing or duplicate {id}"
        );
    }
    assert!(
        fixture.requests.try_recv().is_err(),
        "discovery must not invoke a model"
    );
    assert!(
        fixture
            .client
            .list_sessions(&crate::ports::native_history::ListOptions {
                cwd: Some(fixture.spec.cwd.clone()),
                scan_limit: 100,
            })
            .await
            .unwrap()
            .is_empty(),
        "temporary discovery must leave no session"
    );
    fixture.spec.config.model = Some("local/catalog-63".into());
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn("selected last model", &fixture.spec.config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    assert_eq!(
        request(&mut fixture, "selected last model").await["model"],
        "catalog-63"
    );
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires OpenCode 2.0.25+; concurrent discovery in isolated project directories"]
async fn installed_acp_project_catalogs_and_defaults_do_not_leak_between_directories() {
    let mut fixture = installed(false).await;
    let original = configuration(&fixture);
    std::fs::remove_file(fixture.root.path().join("opencode.json")).unwrap();
    let mut specs = Vec::new();
    let projects = ["alpha", "beta", "gamma", "delta"];
    for project in projects {
        let directory = fixture.root.path().join(project);
        std::fs::create_dir(&directory).unwrap();
        let mut config = original.clone();
        config["model"] = json!(format!("local/{project}"));
        config["providers"]["local"]["models"] = json!({project:{"name":project, "variants":[], "limit":{"context":32000,"output":2000}}});
        write_configuration(&directory, &config);
        specs.push(AgentSessionSpec {
            provider: "opencode".into(),
            cwd: directory
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            config: StoredAgentConfig::default(),
        });
    }
    let (alpha, beta, gamma, delta) = tokio::join!(
        fixture.client.discover(&specs[0].cwd),
        fixture.client.discover(&specs[1].cwd),
        fixture.client.discover(&specs[2].cwd),
        fixture.client.discover(&specs[3].cwd),
    );
    for (index, details) in [alpha, beta, gamma, delta].into_iter().enumerate() {
        let details =
            details.unwrap_or_else(|error| panic!("{} discovery: {error:?}", projects[index]));
        let own = format!("local/{}", projects[index]);
        assert!(
            details
                .models
                .iter()
                .any(|model| model["id"] == own && model["isDefault"] == true)
        );
        for project in projects
            .iter()
            .filter(|project| **project != projects[index])
        {
            assert!(
                !details
                    .models
                    .iter()
                    .any(|model| model["id"] == format!("local/{project}"))
            );
        }
        let mut session = fixture.client.create_session(&specs[index]).await.unwrap();
        session
            .start_turn(&format!("project default {index}"), &specs[index].config)
            .await
            .unwrap();
        completed(session.as_mut()).await;
        assert_eq!(
            request(&mut fixture, &format!("project default {index}")).await["model"],
            projects[index]
        );
        session.close().await.unwrap();
    }
}

#[tokio::test]
#[ignore = "requires OpenCode 2.0.25+; native attachments, images and replay"]
async fn installed_acp_rich_input_reaches_model_and_preserves_client_identity() {
    let mut fixture = installed(false).await;
    let mut config = configuration(&fixture);
    config["providers"]["local"]["models"]["test-model"]["capabilities"] =
        json!({"tools":true,"input":["text","image"],"output":["text"]});
    write_configuration(fixture.root.path(), &config);
    let file = fixture.root.path().join("notes.txt");
    std::fs::write(&file, "isolated uploaded notes").unwrap();
    let prompt = AgentPrompt {
        text:"Read the attached context".into(), client_message_id:Some("rich-client-identity".into()),
        attachments:vec![json!({"type":"text","mimeType":"text/plain","text":"Attachment context sentinel"}),
            json!({"type":"uploaded_file","id":"isolated-upload","fileName":"notes.txt","path":file,"mimeType":"text/plain","size":23})],
        images:vec![PromptImage { mime_type:"image/png".into(), data:"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4//8/AAX+Av4N70a4AAAAAElFTkSuQmCC".into() }],
        ..Default::default()
    };
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_input(&prompt, &fixture.spec.config)
        .await
        .unwrap();
    let items = completed(session.as_mut()).await;
    let received = request(&mut fixture, "Read the attached context").await;
    let messages = received["messages"].to_string();
    assert!(messages.contains("Attachment context sentinel"));
    assert!(messages.contains("notes.txt"));
    assert!(messages.contains("image_url"), "received {received}");
    assert!(messages.contains("data:image/png;base64,"));
    assert!(
        items
            .iter()
            .any(|entry| entry.item["type"] == "user_message"
                && entry.item["clientMessageId"] == "rich-client-identity")
    );
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let history = fixture
        .client
        .history(&handle, &fixture.spec.cwd)
        .await
        .unwrap();
    assert_replay(&items, &history);
}

#[tokio::test]
#[ignore = "requires OpenCode 2.0.25+; cancel a model request already admitted by native CLI"]
async fn installed_acp_inflight_cancellation_settles_and_allows_followup() {
    let mut fixture = installed(false).await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    let turn = session
        .start_turn("Hold native request", &fixture.spec.config)
        .await
        .unwrap();
    request(&mut fixture, "Hold native request").await;
    session.cancel_turn(&turn).await.unwrap();
    loop {
        match event(session.as_mut()).await {
            AgentTurnEvent::Cancelled => break,
            AgentTurnEvent::Completed(_) | AgentTurnEvent::Failed => {
                panic!("cancellation was not confirmed")
            }
            _ => {}
        }
    }
    assert_eq!(session.pending_permissions(), [] as [Value; 0]);
    session
        .start_turn("continue after cancellation", &fixture.spec.config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    assert!(
        request(&mut fixture, "continue after cancellation").await["messages"]
            .to_string()
            .contains("continue after cancellation")
    );
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires OpenCode 2.0.25+; deterministic native model authentication failure"]
async fn installed_acp_model_authentication_error_settles_without_false_success() {
    let mut fixture = installed(false).await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn("Reject native authentication", &fixture.spec.config)
        .await
        .unwrap();
    request(&mut fixture, "Reject native authentication").await;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match session.poll_turn() {
                Err(AgentSessionError::Failed | AgentSessionError::Rejected)
                | Ok(Some(AgentTurnEvent::Failed)) => break,
                Ok(Some(AgentTurnEvent::Completed(_) | AgentTurnEvent::Cancelled)) => {
                    panic!("model error was reported as success or cancellation")
                }
                Err(error) => panic!("unexpected error: {error:?}"),
                _ => tokio::time::sleep(Duration::from_millis(5)).await,
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(session.pending_permissions(), [] as [Value; 0]);
    let _ = session.close().await;
}
