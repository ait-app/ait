use model::automation::Root;
use serde_json::{Value, json};

use super::*;
use crate::test_support::{Workspaces, address};

/// One way of widening a creation request.
type Change = Box<dyn Fn(&mut Value)>;

fn enable(fixture: &Workspaces) {
    fixture
        .api
        .shared
        .automation
        .set(Settings {
            enabled: true,
            roots: vec![Root {
                path: fixture.shared_path().to_string_lossy().into_owned(),
                label: None,
            }],
            providers: vec!["codex".to_owned()],
        })
        .expect("valid settings");
}

fn create(cwd: &str) -> Value {
    json!({
        "idempotencyKey": "automation:1",
        "config": {
            "provider": "codex",
            "cwd": cwd,
            "title": "fix",
            "mcpServers": {"ait_hub": {"type": "http", "url": "https://center.test/v1/automation/mcp",
                "headers": {"Authorization": "Bearer t"}}},
        },
        "labels": {"automationConversation": "c1"},
    })
}

async fn decide(fixture: &Workspaces, method: &str, params: Value) -> Result<Value, ErrorCode> {
    let mut incoming = Request {
        id: "r1".to_owned(),
        method: method.to_owned(),
        params,
    };
    request(&fixture.api.shared, &mut incoming)
        .await
        .map(|()| incoming.params)
}

#[tokio::test]
async fn creation_is_confined_to_roots_and_marked() {
    let fixture = Workspaces::new(address());
    enable(&fixture);
    let inside = fixture.shared_path().join("nested");
    std::fs::create_dir(&inside).expect("nested directory");
    let params = decide(
        &fixture,
        "agent.create.request",
        create(&inside.to_string_lossy()),
    )
    .await
    .expect("admitted");
    assert_eq!(params["labels"][ORIGIN_LABEL], ORIGIN);
    assert_eq!(params["labels"]["automationConversation"], "c1");
    assert_eq!(params["config"]["cwd"], json!(inside.to_string_lossy()));

    let outside = fixture.path("other").to_string_lossy().into_owned();
    assert_eq!(
        decide(&fixture, "agent.create.request", create(&outside)).await,
        Err(ErrorCode::Forbidden)
    );
    let escape = format!("{}/../other", fixture.shared_path().to_string_lossy());
    assert_eq!(
        decide(&fixture, "agent.create.request", create(&escape)).await,
        Err(ErrorCode::Forbidden),
        "paths are compared after resolving .."
    );
}

#[tokio::test]
async fn creation_rejects_extra_powers() {
    let fixture = Workspaces::new(address());
    enable(&fixture);
    let cwd = fixture.shared_path().to_string_lossy().into_owned();
    let cases: [(&str, Change); 7] = [
        ("environment", Box::new(|p| p["env"] = json!({"A":"1"}))),
        ("workspace", Box::new(|p| p["workspaceId"] = json!("ws"))),
        (
            "provider",
            Box::new(|p| p["config"]["provider"] = json!("claude")),
        ),
        (
            "system prompt",
            Box::new(|p| p["config"]["systemPrompt"] = json!("x")),
        ),
        (
            "other MCP server",
            Box::new(|p| {
                p["config"]["mcpServers"]["shell"] = json!({"type":"stdio","command":"sh"});
            }),
        ),
        (
            "bot label",
            Box::new(|p| p["labels"]["ait.bot.id"] = json!("b")),
        ),
        (
            "guest label",
            Box::new(|p| p["labels"]["ait.guest.share"] = json!("s")),
        ),
    ];
    for (name, change) in cases {
        let mut params = create(&cwd);
        change(&mut params);
        assert_eq!(
            decide(&fixture, "agent.create.request", params).await,
            Err(ErrorCode::Forbidden),
            "{name}"
        );
    }
    let mut keyless = create(&cwd);
    keyless
        .as_object_mut()
        .expect("object")
        .remove("idempotencyKey");
    assert_eq!(
        decide(&fixture, "agent.create.request", keyless).await,
        Err(ErrorCode::InvalidMessage)
    );
}

#[tokio::test]
async fn only_automation_methods_on_known_agents_pass() {
    let fixture = Workspaces::new(address());
    enable(&fixture);
    for method in [
        "terminal.create.request",
        "agent.permission.resolve.request",
        "automation.settings.set.request",
        "workspace.archive.request",
    ] {
        assert_eq!(
            decide(&fixture, method, json!({})).await,
            Err(ErrorCode::Forbidden),
            "{method}"
        );
    }
    // Without an Agent service no Agent belongs to automation.
    assert_eq!(
        decide(
            &fixture,
            "agent.message.send.request",
            json!({"agentId":"a1","text":"hi","messageId":"automation:1"})
        )
        .await,
        Err(ErrorCode::Forbidden)
    );
    assert_eq!(
        decide(
            &fixture,
            "agent.message.send.request",
            json!({"agentId":"a1","text":"hi","messageId":"m","images":[]})
        )
        .await,
        Err(ErrorCode::Forbidden),
        "text only"
    );
    assert_eq!(
        decide(
            &fixture,
            "agent.message.send.request",
            json!({"agentId":"a1","text":"hi"})
        )
        .await,
        Err(ErrorCode::InvalidMessage),
        "a retry identity is required"
    );
}

#[tokio::test]
async fn nothing_passes_while_disabled() {
    let fixture = Workspaces::new(address());
    let cwd = fixture.shared_path().to_string_lossy().into_owned();
    assert_eq!(
        decide(&fixture, "agent.create.request", create(&cwd)).await,
        Err(ErrorCode::Forbidden)
    );
}
