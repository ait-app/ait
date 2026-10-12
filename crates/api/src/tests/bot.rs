use std::pin::Pin;

use ::bot::ports::{Credentials, Executor};
use ::bot::service::{Bots, Dependencies};
use domain::BotId;
use domain::bot::SessionOwner;
use model::workspace::registry::{ProjectRegistry, WorkspaceRegistry};
use persistence::storage::bot::FileStore;
use persistence::storage::registry::{FileBackedProjectRegistry, FileBackedWorkspaceRegistry};
use provider::service::bot_execution::Action;

use super::*;

#[derive(Debug)]
struct Native;
impl Executor for Native {
    fn execute(
        &self,
        _: SessionOwner,
        _: Action,
    ) -> Pin<Box<dyn Future<Output = Result<Value, model::ErrorCode>> + Send + '_>> {
        Box::pin(async { Ok(json!({"status":"idle","lastMessage":"Bot answer"})) })
    }
}
impl Credentials for Native {
    fn resolve(&self, _: &str) -> Result<SecretString, model::ErrorCode> {
        Err(model::ErrorCode::BotIo)
    }
}

fn bots(root: &std::path::Path) -> Bots {
    let projects = FileBackedProjectRegistry::new(root.join("projects.json"));
    let workspaces = FileBackedWorkspaceRegistry::new(root.join("workspaces.json"));
    let cwd = root.to_str().unwrap();
    projects.upsert(&serde_json::from_value(json!({"projectId":"project-1","rootPath":cwd,"kind":"non_git","displayName":"project","createdAt":"2026-10-12T00:00:00Z","updatedAt":"2026-10-12T00:00:00Z","archivedAt":null})).unwrap()).unwrap();
    workspaces.upsert(&serde_json::from_value(json!({"workspaceId":"wks_0123456789abcdef","projectId":"project-1","cwd":cwd,"kind":"local_checkout","displayName":"workspace","createdAt":"2026-10-12T00:00:00Z","updatedAt":"2026-10-12T00:00:00Z","archivedAt":null})).unwrap(),domain::workspace::registry::WorkspaceMutationContext::default()).unwrap();
    Bots::spawn(Dependencies {
        store: Arc::new(FileStore::new(root.join("bots"))),
        workspaces: Arc::new(workspaces),
        projects: Arc::new(projects),
        executor: Arc::new(Native),
        credentials: Arc::new(Native),
    })
    .unwrap()
}

async fn connect(fixture: &Fixture) -> Socket {
    let mut socket = fixture.socket().await;
    let mut offer = hello();
    offer["capabilities"] = json!(
        ::bot::capabilities::implemented_methods()
            .map(|method| method.name)
            .collect::<Vec<_>>()
    );
    send(&mut socket, offer).await;
    let reply = receive(&mut socket).await;
    assert!(
        reply["info"]["features"]
            .as_array()
            .unwrap()
            .contains(&json!("workspace-bots-v1"))
    );
    socket
}

#[tokio::test]
async fn installed_bot_rpc_survives_ui_disconnect_and_returns_durable_history() {
    let root = tempfile::tempdir().unwrap();
    let fixture = Fixture::with_services(Services {
        bot: Some(bots(root.path())),
        ..Services::default()
    })
    .await;
    let mut socket = connect(&fixture).await;
    let id = BotId::generate();
    let created = request(&mut socket,"bot.create.request",json!({"botId":id,"workspaceId":"wks_0123456789abcdef","name":"assistant","execution":{"provider":"test"}})).await;
    assert_eq!(created["type"], "response");
    let opened = request(
        &mut socket,
        "bot.conversation.open.request",
        json!({"botId":id}),
    )
    .await;
    let conversation = opened["result"]["conversation"]["id"].clone();
    let accepted = request(
        &mut socket,
        "bot.message.send.request",
        json!({"botId":id,"conversationId":conversation,"messageId":"stable","text":"hello"}),
    )
    .await;
    assert_eq!(accepted["result"]["accepted"], true);
    socket.close(None).await.unwrap();
    let mut restored = connect(&fixture).await;
    let history = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let value = request(
                &mut restored,
                "bot.history.get.request",
                json!({"botId":id,"conversationId":conversation}),
            )
            .await;
            if value["result"]["turns"][0]["status"] == "completed" {
                break value;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(history["result"]["turns"][0]["reply"], "Bot answer");
    assert!(!history.to_string().contains("sessionId"));
    restored.close(None).await.unwrap();
    fixture.stop().await;
}
