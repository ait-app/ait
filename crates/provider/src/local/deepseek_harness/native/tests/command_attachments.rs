use serde_json::{Value, json};

use super::fixture::{Fixture, next};
use crate::ports::agent_session::{AgentClient, AgentSessionError, AgentTurnEvent};
use crate::protocol::prompt::{AgentPrompt, PromptImage};

fn advertise(fixture: &Fixture) {
    fixture.reply(
        "commands/list",
        json!([{"name":"plugin_action","description":"Native action",
        "input":{"hint":"<value>","attachments":true}}]),
    );
}

async fn file(fixture: &Fixture) -> Value {
    let path = std::path::Path::new(&fixture.spec.cwd).join("file with ?&.bin");
    let bytes = [0, 255, 17, 0, 3].repeat(40_000);
    tokio::fs::write(&path, &bytes).await.unwrap();
    json!({"type":"uploaded_file","id":"upload_fixture","path":path,
        "fileName":"file with ?&.bin","mimeType":"application/octet-stream","size":bytes.len()})
}

#[tokio::test]
async fn native_command_attachments_use_exact_bytes_and_scoped_receipts_without_model_input() {
    let fixture = Fixture::new().await;
    advertise(&fixture);
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    let attachment = file(&fixture).await;
    let prompt = AgentPrompt {
        text: "/plugin_action \n  preserve arguments  ".into(),
        images: vec![PromptImage {
            data: "aGVsbG8=".into(),
            mime_type: "image/png".into(),
        }],
        attachments: vec![attachment],
        ..AgentPrompt::default()
    };
    session.out_of_band(&prompt).await.unwrap();
    let completion = session.take_command_completion().unwrap();
    assert!(session.take_command_completion().is_none());
    completion.await.unwrap();
    let uploaded = &fixture.requests("file_upload")[0];
    assert_eq!(uploaded["sessionId"], "session");
    assert_eq!(uploaded["name"], "file with ?&.bin");
    assert_eq!(uploaded["bytes"], json!([0, 255, 17, 0, 3].repeat(40_000)));
    let submitted = &fixture.requests("commands/execute")[0];
    assert_eq!(submitted["agentId"], "session");
    assert_eq!(submitted["line"], prompt.text);
    assert_eq!(
        submitted["submittedAttachments"],
        json!([
            {"type":"image","mediaType":"image/png","data":"aGVsbG8="},
            {"type":"file","receiptId":"native-receipt"}
        ])
    );
    assert!(fixture.requests("session/prompt").is_empty());
    session.close().await.unwrap();
}

#[tokio::test]
async fn unsupported_commands_context_objects_and_invalid_files_reject_before_any_upload() {
    let fixture = Fixture::new().await;
    let attachment = file(&fixture).await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    let mut prompt = AgentPrompt {
        text: "/plugin_action".into(),
        attachments: vec![attachment.clone()],
        ..AgentPrompt::default()
    };
    assert_eq!(
        session.out_of_band(&prompt).await,
        Err(AgentSessionError::Rejected)
    );
    advertise(&fixture);
    session.refresh_commands().await.unwrap();
    let mut changed = attachment.clone();
    changed["size"] = json!(1);
    for attachments in [
        vec![
            attachment.clone(),
            json!({"type":"text","mimeType":"text/plain","text":"context"}),
        ],
        vec![attachment.clone(), changed],
        vec![
            json!({"type":"uploaded_file","id":"missing","path":format!("{}/missing",fixture.spec.cwd),
            "fileName":"missing","mimeType":"text/plain","size":0}),
        ],
    ] {
        prompt.attachments = attachments;
        assert_eq!(
            session.out_of_band(&prompt).await,
            Err(AgentSessionError::Rejected)
        );
        assert!(session.take_command_completion().is_none());
    }
    assert!(fixture.requests("file_upload").is_empty());
    assert!(fixture.requests("commands/execute").is_empty());
    session.close().await.unwrap();
}

#[tokio::test]
async fn native_attachment_handler_errors_are_rejections_and_upload_failures_never_execute_commands()
 {
    for failure in ["handler", "500", "malformed", "large", "rejected", "size"] {
        let fixture = Fixture::new().await;
        advertise(&fixture);
        let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
        if failure == "handler" {
            fixture.reply("commands/execute", json!({"commandId":"command-1","result":{"kind":"error","text":"Invalid subcommand"}}));
        } else if failure == "size" {
            fixture.reply(
                "file_upload",
                json!({"receiptId":"wrong","file":{"bytes":1}}),
            );
        } else {
            fixture.fail("file_upload", failure);
        }
        let prompt = AgentPrompt {
            text: "/plugin_action invalid".into(),
            attachments: vec![file(&fixture).await],
            ..AgentPrompt::default()
        };
        session.out_of_band(&prompt).await.unwrap();
        let result = session.take_command_completion().unwrap().await;
        assert!(result.is_err(), "{failure}");
        if matches!(failure, "handler" | "rejected") {
            assert_eq!(result, Err(AgentSessionError::Rejected));
        }
        assert_eq!(
            fixture.requests("commands/execute").len(),
            usize::from(failure == "handler")
        );
        assert!(session.failure_message().is_none());
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn attachment_command_can_ask_questions_while_final_admission_is_pending() {
    let fixture = Fixture::new().await;
    advertise(&fixture);
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    let release = fixture.hold_command();
    let prompt = AgentPrompt {
        text: "/plugin_action".into(),
        attachments: vec![file(&fixture).await],
        ..AgentPrompt::default()
    };
    session.out_of_band(&prompt).await.unwrap();
    let mut completion = session.take_command_completion().unwrap();
    fixture.history(
        1,
        "command/run",
        json!({"commandId":"command-1","name":"plugin_action","args":"","source":{"kind":"user"}}),
    );
    fixture.interaction(json!({"type":"waterfall","agentId":"session","eventId":"question",
        "event":"user-questions/request","request":{"questions":[{"id":"direction","question":"Proceed?","options":[{"label":"Yes"}]}]}}));
    let question = loop {
        if let AgentTurnEvent::PermissionRequested(question) = next(session.as_mut()).await {
            break question;
        }
    };
    assert!(futures_util::poll!(&mut completion).is_pending());
    session
        .respond_permission(
            question["id"].as_str().unwrap(),
            &json!({"behavior":"allow",
        "updatedInput":{"answers":{"direction":["Yes"]}}}),
        )
        .await
        .unwrap();
    release.send(()).unwrap();
    completion.await.unwrap();
    assert_eq!(fixture.requests("$events/result").len(), 1);
    assert!(fixture.requests("session/prompt").is_empty());
    session.close().await.unwrap();
}

#[tokio::test]
async fn execution_lane_remains_available_and_rejected_attachment_commands_keep_failed_admission() {
    let fixture = Fixture::new().await;
    advertise(&fixture);
    let mut host = crate::test_support::Fixture::new();
    host.cwd = std::path::PathBuf::from(&fixture.spec.cwd);
    let (execution, _) = crate::test_support::worker_with_client(
        &host,
        model::creation::Creations::default(),
        Box::new(fixture.client.clone()),
    );
    let environment: std::collections::BTreeMap<_, _> =
        fixture.client.environment.entries().collect();
    let created = execution
        .execute(
            "agent.create.request",
            json!({"config":{
        "provider":"deepseek-harness","cwd":host.cwd},"env":environment}),
        )
        .await
        .unwrap();
    let id = created["agentId"].as_str().unwrap().to_owned();
    fixture.reply(
        "commands/execute",
        json!({"commandId":"command-1","result":{"kind":"error","text":"Revise the attachment"}}),
    );
    let release = fixture.hold_command();
    let request = json!({"agentId":id,"messageId":"attachment-command","text":"/plugin_action",
        "attachments":[file(&fixture).await]});
    let operation = {
        let execution = execution.clone();
        let request = request.clone();
        tokio::spawn(async move {
            execution
                .execute("agent.message.send.request", request)
                .await
        })
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while fixture.requests("commands/execute").is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!operation.is_finished());
    fixture.history(
        1,
        "command/run",
        json!({"commandId":"command-1","name":"plugin_action","source":{"kind":"user"}}),
    );
    fixture.interaction(json!({"type":"waterfall","agentId":"session","eventId":"question",
        "event":"user-questions/request","request":{"questions":[{"id":"direction","question":"Proceed?","options":[{"label":"Yes"}]}]}}));
    let question = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let current = execution
                .execute("agent.get.request", json!({"agentId":id}))
                .await
                .unwrap();
            if let Some(question) = current["agent"]["pendingPermissions"]
                .as_array()
                .and_then(|items| items.first())
            {
                break question.clone();
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    execution
        .execute(
            "agent.permission.resolve.request",
            json!({"agentId":id,
        "requestId":question["id"],"response":{"behavior":"allow",
        "updatedInput":{"answers":{"direction":["Yes"]}}}}),
        )
        .await
        .unwrap();
    assert!(!operation.is_finished());
    release.send(()).unwrap();
    let response = operation.await.unwrap().unwrap();
    assert_eq!(response["accepted"], false);
    fixture.reply(
        "commands/execute",
        json!({"commandId":"command-2","result":{"kind":"success"}}),
    );
    let repeated = execution
        .execute("agent.message.send.request", request.clone())
        .await
        .unwrap();
    assert_eq!(repeated["accepted"], true);
    let accepted_retry = execution
        .execute("agent.message.send.request", request)
        .await
        .unwrap();
    assert_eq!(accepted_retry["accepted"], true);
    assert_eq!(fixture.requests("commands/execute").len(), 2);
    assert!(fixture.requests("session/prompt").is_empty());
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn initial_attachment_command_does_not_complete_creation_before_native_admission() {
    let fixture = Fixture::new().await;
    advertise(&fixture);
    let mut host = crate::test_support::Fixture::new();
    host.cwd = std::path::PathBuf::from(&fixture.spec.cwd);
    let (execution, _) = crate::test_support::worker_with_client(
        &host,
        model::creation::Creations::default(),
        Box::new(fixture.client.clone()),
    );
    let environment: std::collections::BTreeMap<_, _> =
        fixture.client.environment.entries().collect();
    let release = fixture.hold_command();
    fixture.reply(
        "commands/execute",
        json!({"commandId":"command-1","result":{"kind":"error","text":"Revise the attachment"}}),
    );
    let initial = json!({"config":{"provider":"deepseek-harness","cwd":host.cwd},
        "env":environment,"initialPrompt":"/plugin_action","attachments":[file(&fixture).await]});
    let operation = {
        let execution = execution.clone();
        tokio::spawn(async move { execution.execute("agent.create.request", initial).await })
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while fixture.requests("commands/execute").is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!operation.is_finished());
    release.send(()).unwrap();
    assert!(operation.await.unwrap().is_err());
    assert!(fixture.requests("session/prompt").is_empty());
    execution.shutdown().await.unwrap();
}
