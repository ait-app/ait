use serde_json::json;

use super::{
    archive::ARCHIVE,
    fixture::{Fixture, next},
};
use crate::{
    ports::agent_session::{AgentClient, AgentTurnEvent},
    protocol::prompt::AgentPrompt,
};

fn advertise(fixture: &Fixture, definition: &str) {
    fixture.reply(
        "commands/list",
        json!([{"name":"export","description":"Session archive",
        "definitionId":definition}]),
    );
}

#[tokio::test]
async fn native_export_streams_tree_and_restores_download_on_the_same_journal_card() {
    let mut fixture = Fixture::new().await;
    fixture.client = fixture
        .client
        .clone()
        .with_image_directory(std::path::PathBuf::from(&fixture.spec.cwd).join("artifacts"));
    advertise(&fixture, "@deepseek-ai/dsh-session-log-export");
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    let release = fixture.hold_command();
    session
        .out_of_band(&AgentPrompt::text("/export"))
        .await
        .unwrap();
    let completion = session.take_command_completion().unwrap();
    fixture.history(
        1,
        "command/run",
        json!({"commandId":"command-1","name":"export","source":{"kind":"user"}}),
    );
    fixture.history(
        2,
        "command/done",
        json!({"commandId":"command-1","kind":"success","text":"Download requested"}),
    );
    let before = loop {
        if let AgentTurnEvent::Timeline(item) = next(session.as_mut()).await
            && item.item["type"] == "provider_command"
        {
            break item;
        }
    };
    assert!(before.item.get("file").is_none());
    release.send(()).unwrap();
    completion.await.unwrap();
    let delivered = loop {
        if let AgentTurnEvent::Timeline(item) = next(session.as_mut()).await
            && item.item.get("file").is_some()
        {
            break item;
        }
    };
    assert_eq!(before.key, delivered.key);
    assert_eq!(before.item["id"], delivered.item["id"]);
    let artifact = &delivered.item["file"];
    let path = std::path::Path::new(artifact["directory"].as_str().unwrap())
        .join(artifact["fileName"].as_str().unwrap());
    assert_eq!(tokio::fs::read(path).await.unwrap(), ARCHIVE);
    assert_eq!(
        fixture.requests("session_export"),
        vec![json!({"sessionId":"session","includeDescendants":"true"})]
    );
    assert!(fixture.requests("session/prompt").is_empty());
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let history = fixture
        .client
        .history(&handle, &fixture.spec.cwd)
        .await
        .unwrap();
    assert_eq!(
        history
            .iter()
            .filter(|item| item.key == delivered.key)
            .count(),
        1
    );
    assert_eq!(
        history
            .iter()
            .find(|item| item.key == delivered.key)
            .unwrap()
            .item,
        delivered.item
    );
}

#[tokio::test]
async fn failed_native_downloads_do_not_leave_a_deliverable_archive_or_pollute_model_diagnostics() {
    for failure in ["500", "html", "truncated", "large"] {
        let mut fixture = Fixture::new().await;
        fixture.client = fixture
            .client
            .clone()
            .with_image_directory(std::path::PathBuf::from(&fixture.spec.cwd).join("artifacts"));
        advertise(&fixture, "@deepseek-ai/dsh-session-log-export");
        fixture.fail("session_export", failure);
        let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
        session
            .out_of_band(&AgentPrompt::text("/export"))
            .await
            .unwrap();
        assert!(
            session.take_command_completion().unwrap().await.is_err(),
            "{failure}"
        );
        let _ = session.poll_turn();
        assert!(session.failure_message().is_none());
        let archive = std::path::Path::new(&fixture.spec.cwd).join("artifacts/dsh-exports");
        if archive.exists() {
            for child in std::fs::read_dir(archive).unwrap() {
                let child = child.unwrap();
                assert_eq!(
                    std::fs::read_dir(child.path()).unwrap().count(),
                    0,
                    "{failure}"
                );
            }
        }
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn custom_same_name_commands_and_native_rejections_never_download_an_archive() {
    let fixture = Fixture::new().await;
    advertise(&fixture, "plugin:custom-export");
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .out_of_band(&AgentPrompt::text("/export"))
        .await
        .unwrap();
    assert!(session.take_command_completion().is_none());
    advertise(&fixture, "@deepseek-ai/dsh-session-log-export");
    session.refresh_commands().await.unwrap();
    fixture.reply(
        "commands/execute",
        json!({"commandId":"command-2","result":{"kind":"error","text":"No paths accepted"}}),
    );
    session
        .out_of_band(&AgentPrompt::text("/export /tmp/target"))
        .await
        .unwrap();
    assert!(session.take_command_completion().unwrap().await.is_err());
    assert!(fixture.requests("session_export").is_empty());
    session.close().await.unwrap();
}
