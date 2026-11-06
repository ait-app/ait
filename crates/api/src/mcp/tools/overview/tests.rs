use std::collections::BTreeSet;

use std::sync::LazyLock;

use super::*;

static CHECKOUT: LazyLock<Value> = LazyLock::new(|| json!({"currentBranch":"main","isDirty":true}));
static MEMORY: LazyLock<Value> = LazyLock::new(|| json!({"text":"hello","revision":"r0"}));

fn agent(index: usize, workspace: &str, scoped: bool) -> Value {
    let labels = if scoped {
        json!({BOT_SCOPE_LABEL:"wks_root"})
    } else {
        json!({})
    };
    json!({"id":format!("agent-{index}"),"title":"T".repeat(60),"provider":"codex","status":"idle",
        "workspaceId":workspace,"labels":labels,"requiresAttention":false,
        "updatedAt":"2026-11-06T00:00:00Z","lastMessage":"m".repeat(4096)})
}

fn sources<'a>(
    own: &'a [Value],
    all: &'a [Value],
    schedules: &'a [Value],
    workspaces: &'a BTreeSet<String>,
    permissions: &'a BotPermissions,
) -> Sources<'a> {
    Sources {
        root: "wks_root",
        checkout: &CHECKOUT,
        own,
        all,
        all_complete: true,
        schedules,
        workspaces,
        memory: &MEMORY,
        permissions,
    }
}

#[test]
fn workspace_overview_is_bounded_and_scoped() {
    let own: Vec<_> = (0..50)
        .map(|index| agent(index, "wks_root", true))
        .collect();
    let mut all = own.clone();
    all.push(agent(90, "wks_root", false));
    let workspaces = BTreeSet::from(["wks_root".to_owned()]);
    let schedules = [
        json!({"id":"s1","workspaceId":"wks_root","botScope":"wks_root","name":"n","status":"active"}),
        json!({"id":"s2","workspaceId":"wks_root","name":"user","status":"active"}),
        json!({"id":"foreign","workspaceId":"wks_other","botScope":"wks_root","status":"active"}),
    ];
    let permissions = BotPermissions::default();
    let overview = project(&sources(&own, &all, &schedules, &workspaces, &permissions));
    let text = overview.to_string();
    assert!(text.len() <= MAX_BYTES, "{}", text.len());
    assert_eq!(overview["truncated"], true);
    assert_eq!(overview["botAgents"].as_array().unwrap().len(), 20);
    assert!(!text.contains("foreign"));
    assert!(overview["notifications"].is_null());
    assert_eq!(overview["otherAgents"]["count"], 1);
    assert_eq!(overview["schedules"]["otherCount"], 1);
    assert_eq!(overview["schedules"]["bot"].as_array().unwrap().len(), 1);
    assert_eq!(overview["memory"], json!({"revision":"r0","bytes":5}));
    assert_eq!(overview["workspace"]["branch"], "main");
    assert_eq!(overview["permissions"]["groups"][0], "observe");
}

#[test]
fn small_overview_keeps_last_messages_and_is_not_truncated() {
    let own = [agent(0, "wks_root", true), agent(1, "wks_root", true)];
    let workspaces = BTreeSet::from(["wks_root".to_owned()]);
    let permissions = BotPermissions::default();
    let overview = project(&sources(&own, &own, &[], &workspaces, &permissions));
    assert_eq!(overview["truncated"], false);
    assert_eq!(
        overview["botAgents"][0]["lastMessage"]
            .as_str()
            .unwrap()
            .len(),
        MAX_MESSAGE
    );
}

#[test]
fn oversized_documents_lose_last_messages_before_entries() {
    let own: Vec<_> = (0..20)
        .map(|index| agent(index, "wks_root", true))
        .collect();
    let workspaces = BTreeSet::from(["wks_root".to_owned()]);
    let permissions = BotPermissions::default();
    let overview = project(&sources(&own, &own, &[], &workspaces, &permissions));
    assert!(overview.to_string().len() <= MAX_BYTES);
    assert_eq!(overview["botAgents"].as_array().unwrap().len(), 20);
    assert!(overview["botAgents"][0].get("lastMessage").is_none());
    assert_eq!(overview["truncated"], false);
}

#[test]
fn clip_respects_character_boundaries() {
    assert_eq!(clip(&json!("你好"), 4), json!("你"));
    assert_eq!(clip(&json!("abc"), 8), json!("abc"));
}
