use std::process::Command;

use domain::workspace::registry::WorkspaceMutationContext;
use filesystem::forge::local::forge::LocalForge;
use persistence::storage::registry::FileBackedWorkspaceRegistry;
use serde_json::json;

use super::*;

const TICKET: &str = "44444444-4444-4444-8444-444444444444";
const OTHER: &str = "55555555-5555-4555-8555-555555555555";

fn record(id: &str, ticket: &str, archived: Option<&str>) -> PersistedAgentRuntimeRecord {
    serde_json::from_value(json!({"id":id,"provider":"codex","cwd":"/tmp",
        "workspaceId":"wks_1","createdAt":"2026-11-14T00:00:00Z",
        "updatedAt":"2026-11-14T00:00:00Z","title":"Fix","archivedAt":archived,
        "labels":{BOT_TICKET_LABEL:ticket}}))
    .unwrap()
}

#[test]
fn live_workers_are_the_unarchived_agents_labelled_with_the_ticket() {
    let records = [
        record("a1", TICKET, None),
        record("a2", TICKET, Some("2026-11-14T01:00:00Z")),
        record("a3", OTHER, None),
        record("a4", TICKET, Some("")),
    ];

    let live = live(records.iter(), TICKET.parse().unwrap());

    assert_eq!(live, ["a1", "a4"]);
}

#[test]
fn only_what_tool_calls_sent_is_collected() {
    let page = json!({"entries":[
        {"item":{"type":"tool_call","detail":{"type":"shell",
            "command":"gh pr comment 12 --body 'Done'","output":"posted a quoted reply"}}},
        {"item":{"type":"tool_call","detail":{"type":"read","filePath":"/r/notes.md",
            "content":"text the worker only read"}}},
        {"item":{"type":"tool_call","detail":{"type":"unknown",
            "input":{"body":"Addressed the review"},"output":null}}},
        {"item":{"type":"assistant_message","text":"Pushed the fix"}},
        {"item":{"type":"user_message","text":"the owner's words"}}
    ]});
    let mut inputs = Vec::new();

    tool_inputs(&page, &mut inputs);

    assert!(inputs.contains(&"gh pr comment 12 --body 'Done'".to_owned()));
    assert!(inputs.contains(&"Addressed the review".to_owned()));
    for absent in [
        "posted a quoted reply",
        "text the worker only read",
        "Pushed the fix",
        "the owner's words",
    ] {
        assert!(!inputs.iter().any(|input| input == absent), "{absent}");
    }
}

#[test]
fn only_an_agent_without_a_running_turn_takes_an_input() {
    assert!(idle(AgentRuntimeStatus::Idle));
    assert!(idle(AgentRuntimeStatus::Closed));
    assert!(idle(AgentRuntimeStatus::Error));
    assert!(!idle(AgentRuntimeStatus::Running));
    assert!(!idle(AgentRuntimeStatus::Initializing));
}

#[tokio::test]
async fn reading_a_pull_request_of_an_unknown_workspace_or_without_a_forge_is_unavailable() {
    let root = tempfile::tempdir().unwrap();
    let workspaces = FileBackedWorkspaceRegistry::new(root.path().join("workspaces.json"));
    workspaces.initialize().unwrap();
    let checkout = root.path().join("repo");
    std::fs::create_dir(&checkout).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(&checkout)
            .status()
            .unwrap()
            .success()
    );
    workspaces
        .upsert(
            &serde_json::from_value(json!({"workspaceId":"wks_1","projectId":"p",
                "cwd":checkout,"kind":"local_checkout","displayName":"w",
                "createdAt":"2026-11-14T00:00:00Z","updatedAt":"2026-11-14T00:00:00Z",
                "archivedAt":null}))
            .unwrap(),
            WorkspaceMutationContext::default(),
        )
        .unwrap();
    let pulls = ForgePulls {
        forge: Arc::new(Mutex::new(Forge::new(Box::new(LocalForge::new())))),
        workspaces: Arc::new(workspaces),
    };

    let unknown = pulls.read("wks_gone", 12).await;
    let no_remote = pulls.read("wks_1", 12).await;

    assert!(
        matches!(unknown, Err(PullReadError::Unavailable(_))),
        "{unknown:?}"
    );
    assert!(
        matches!(no_remote, Err(PullReadError::Unavailable(_))),
        "{no_remote:?}"
    );
}
