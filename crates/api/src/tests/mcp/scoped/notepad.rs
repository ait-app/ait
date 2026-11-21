//! The Workspace notepad tools, retired tool names and `heartbeat_list` on the MCP route
//! (ADR-295, ADR-296).

use super::support::{codex_modes, forbidden, rejected, structured};
use super::*;

/// The structured content of a tool call that reported a business refusal.
fn refused(response: &Value) -> &Value {
    assert_eq!(response["result"]["isError"], true, "{response}");
    &response["result"]["structuredContent"]
}

fn text(response: &Value) -> &str {
    assert_eq!(response["result"]["isError"], true, "{response}");
    response["result"]["content"][0]["text"].as_str().unwrap()
}

fn with_notepad() -> Value {
    let mut permissions = codex_modes(&["auto"]);
    permissions["groups"]
        .as_array_mut()
        .unwrap()
        .push(json!("notepad"));
    permissions
}

#[tokio::test]
async fn host_callers_share_one_notepad_per_workspace() {
    let scoped = Scoped::start().await;
    let agent = scoped.agent(json!({})).await;
    let root = scoped.workspace.clone();

    let unplaced = scoped.call(TOKEN, None, "notepad_list", json!({})).await;
    assert_eq!(text(&unplaced), model::ErrorCode::InvalidMessage.message());

    let external = scoped
        .call(
            TOKEN,
            None,
            "notepad_write",
            json!({"workspaceId":root,"key":"api-contract","text":"POST /x"}),
        )
        .await;
    assert_eq!(
        structured(&external),
        &json!({"accepted":true,"workspaceId":root,"key":"api-contract","revision":1,
            "notepadRevision":1})
    );
    let question = scoped
        .call(
            AGENT_MCP_TOKEN,
            Some(&agent),
            "notepad_write",
            json!({"key":"questions","text":"Which port?","mode":"append"}),
        )
        .await;
    assert_eq!(structured(&question)["workspaceId"], root);

    let listed = scoped
        .call(TOKEN, Some(&agent), "notepad_list", json!({}))
        .await;
    let listed = structured(&listed);
    assert_eq!(listed["revision"], 2);
    assert_eq!(listed["notes"][0]["key"], "api-contract");
    assert_eq!(listed["notes"][0]["updatedBy"], json!({"kind":"external"}));
    assert_eq!(
        listed["notes"][1]["updatedBy"],
        json!({"kind":"agent","agentId":agent,"title":"Scoped Agent"})
    );
    let since = scoped
        .call(TOKEN, Some(&agent), "notepad_list", json!({"since":1}))
        .await;
    assert_eq!(structured(&since)["notes"].as_array().unwrap().len(), 1);

    let read = scoped
        .call(
            TOKEN,
            Some(&agent),
            "notepad_read",
            json!({"key":"questions"}),
        )
        .await;
    let read = structured(&read);
    assert!(
        read["text"]
            .as_str()
            .unwrap()
            .starts_with("[Scoped Agent · ")
    );
    assert!(read["text"].as_str().unwrap().ends_with("]\nWhich port?"));
    assert_eq!(read["revision"], 1);
    assert_eq!(read["nextOffset"], Value::Null);
    assert_eq!(read["workspaceId"], root);

    let elsewhere = scoped
        .call(
            TOKEN,
            None,
            "notepad_list",
            json!({"workspaceId":scoped.other}),
        )
        .await;
    assert_eq!(structured(&elsewhere)["total"], 0);
    let archived = scoped
        .call(
            TOKEN,
            None,
            "notepad_list",
            json!({"workspaceId":"wks_missing"}),
        )
        .await;
    assert_eq!(
        text(&archived),
        model::ErrorCode::WorkspaceNotFound.message()
    );
    scoped.stop().await;
}

#[tokio::test]
async fn notes_are_read_in_pages_and_changed_only_with_the_revision_read() {
    let scoped = Scoped::start().await;
    let root = scoped.workspace.clone();
    let call = async |tool: &str, mut arguments: Value| {
        arguments["workspaceId"] = json!(root);
        scoped.call(TOKEN, None, tool, arguments).await
    };
    let long = format!("{}{}", "a".repeat(4000), "é".repeat(1000));
    structured(&call("notepad_write", json!({"key":"log","text":long})).await);

    let first = call("notepad_read", json!({"key":"log"})).await;
    let first = structured(&first);
    assert_eq!(
        (&first["chars"], &first["nextOffset"]),
        (&json!(5000), &json!(4000))
    );
    assert_eq!(first["text"].as_str().unwrap().len(), 4000);
    let rest = call(
        "notepad_read",
        json!({"key":"log","offset":4000,"limit":8000}),
    )
    .await;
    let rest = structured(&rest);
    assert_eq!(rest["text"].as_str().unwrap().chars().count(), 1000);
    assert_eq!(rest["nextOffset"], Value::Null);
    let beyond = call("notepad_read", json!({"key":"log","offset":9000})).await;
    assert_eq!(structured(&beyond)["text"], "");

    let absent = call("notepad_read", json!({"key":"none"})).await;
    assert!(
        refused(&absent)["error"]
            .as_str()
            .unwrap()
            .contains("notepad_list")
    );

    let exists = call("notepad_write", json!({"key":"log","text":"short"})).await;
    let exists = refused(&exists);
    assert_eq!(
        (&exists["accepted"], &exists["revision"]),
        (&json!(false), &json!(1))
    );
    assert_eq!(exists["updatedBy"], json!({"kind":"external"}));
    let stale = call("notepad_delete", json!({"key":"log","expected":4})).await;
    assert_eq!(refused(&stale)["revision"], 1);
    structured(
        &call(
            "notepad_write",
            json!({"key":"log","text":"short","expected":1}),
        )
        .await,
    );
    let deleted = call("notepad_delete", json!({"key":"log","expected":2})).await;
    assert_eq!(structured(&deleted)["notepadRevision"], 3);

    let invalid = call("notepad_write", json!({"key":"Not A Key","text":"t"})).await;
    assert_eq!(text(&invalid), model::ErrorCode::InvalidMessage.message());
    rejected(
        &call(
            "notepad_write",
            json!({"key":"k","text":"t","mode":"merge"}),
        )
        .await,
        "Arguments do not match the tool input schema",
    );
    scoped.stop().await;
}

#[tokio::test]
async fn a_bot_and_its_agents_write_only_with_the_notepad_group_and_inside_their_scope() {
    let scoped = Scoped::start().await;
    scoped.setup(codex_modes(&["auto"])).await;
    let owner = scoped.owner_credential().await;
    let child = scoped.subtree_agent().await;
    let subtree = scoped.subtree_credential(&child);
    let write = json!({"key":"plan","text":"step one","mode":"append"});

    for bearer in [owner.as_str(), subtree.as_str()] {
        let tools = scoped.tools(bearer).await;
        assert!(tools.contains("notepad_list") && tools.contains("notepad_read"));
        assert!(!tools.contains("notepad_write") && !tools.contains("notepad_delete"));
        rejected(
            &scoped
                .call(bearer, None, "notepad_write", write.clone())
                .await,
            "Unknown or unavailable tool",
        );
    }

    scoped.permissions(with_notepad()).await;
    let by_bot = scoped
        .call(&owner, None, "notepad_write", write.clone())
        .await;
    assert_eq!(structured(&by_bot)["workspaceId"], scoped.workspace);
    structured(
        &scoped
            .call(&subtree, None, "notepad_write", write.clone())
            .await,
    );
    let read = scoped
        .call(&subtree, None, "notepad_read", json!({"key":"plan"}))
        .await;
    let read = structured(&read);
    assert_eq!(read["createdBy"], json!({"kind":"bot"}));
    assert_eq!(read["updatedBy"]["agentId"], child);
    assert!(
        read["text"]
            .as_str()
            .unwrap()
            .starts_with("[Workspace Bot · ")
    );

    let overview = scoped.call(&owner, None, "bot_overview", json!({})).await;
    assert_eq!(
        structured(&overview)["notepad"],
        json!({"revision":2,"notes":1})
    );

    let mut foreign = write.clone();
    foreign["workspaceId"] = json!(scoped.other);
    forbidden(&scoped.call(&subtree, None, "notepad_write", foreign).await);
    forbidden(
        &scoped
            .call(
                &owner,
                None,
                "notepad_list",
                json!({"workspaceId":scoped.other}),
            )
            .await,
    );
    scoped.stop().await;
}

#[tokio::test]
async fn a_conversation_restricted_to_its_own_work_may_not_use_the_shared_notepad() {
    let scoped = Scoped::start().await;
    scoped.setup(with_notepad()).await;
    let stranger = scoped.bot_credential();
    for (tool, arguments) in [
        ("notepad_list", json!({})),
        ("notepad_read", json!({"key":"plan"})),
        ("notepad_write", json!({"key":"plan","text":"t"})),
        ("notepad_delete", json!({"key":"plan","expected":1})),
    ] {
        let response = scoped.call(&stranger, None, tool, arguments).await;
        assert!(
            text(&response).starts_with("The workspace notepad is shared by the whole workspace"),
            "{tool}: {response}"
        );
    }
    let overview = scoped
        .call(&stranger, None, "bot_overview", json!({}))
        .await;
    assert_eq!(structured(&overview)["notepad"], Value::Null);
    let listed = scoped
        .rpc(
            "workspace.notepad.list.request",
            json!({"workspaceId":scoped.workspace}),
        )
        .await;
    assert_eq!(listed["revision"], 0);
    scoped.stop().await;
}

#[tokio::test]
async fn retired_names_still_call_their_tools_but_are_not_listed() {
    let scoped = Scoped::start().await;
    let agent = scoped.agent(json!({})).await;
    let tools = scoped.tools(TOKEN).await;
    for retired in [
        "list_agents",
        "update_agent",
        "kill_agent",
        "report_progress",
    ] {
        assert!(!tools.contains(retired), "{retired}");
    }
    assert!(tools.contains("agent_list") && tools.contains("agent_archive"));

    structured(&scoped.call(TOKEN, None, "list_agents", json!({})).await);
    let renamed = scoped
        .call(
            TOKEN,
            None,
            "update_agent",
            json!({"agentId":agent,"name":"Renamed"}),
        )
        .await;
    structured(&renamed);
    let status = scoped
        .call(TOKEN, None, "get_agent_status", json!({"agentId":agent}))
        .await;
    assert_eq!(structured(&status)["snapshot"]["title"], "Renamed");
    // The current name takes the current argument only.
    rejected(
        &scoped
            .call(
                TOKEN,
                None,
                "agent_update",
                json!({"agentId":agent,"name":"Again"}),
            )
            .await,
        "Arguments do not match the tool input schema",
    );
    let killed = scoped
        .call(TOKEN, None, "kill_agent", json!({"agentId":agent}))
        .await;
    assert!(structured(&killed)["archivedAt"].is_string());

    rejected(
        &scoped.call(TOKEN, None, "no_such_tool", json!({})).await,
        "Unknown or unavailable tool",
    );
    // A retired name grants nothing its tool does not: the Bot has no terminals group.
    scoped.setup(codex_modes(&["auto"])).await;
    let owner = scoped.owner_credential().await;
    rejected(
        &scoped
            .call(&owner, None, "create_terminal", json!({}))
            .await,
        "Unknown or unavailable tool",
    );
    structured(
        &scoped
            .call(&owner, None, "workspace_overview", json!({}))
            .await,
    );
    scoped.stop().await;
}

#[tokio::test]
async fn heartbeat_list_returns_only_the_callers_heartbeats() {
    let scoped = Scoped::start().await;
    let first = scoped.agent(json!({})).await;
    let second = scoped.agent(json!({})).await;
    let create = json!({"prompt":"check the build","cron":"0 9 * * *"});
    let created = scoped
        .call(TOKEN, Some(&first), "heartbeat_create", create.clone())
        .await;
    let heartbeat = structured(&created)["schedule"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    structured(
        &scoped
            .call(
                TOKEN,
                None,
                "schedule_create",
                json!({"workspaceId":scoped.workspace,"provider":"codex/offline-model",
                    "prompt":"report","cron":"0 9 * * *"}),
            )
            .await,
    );

    let own = scoped
        .call(TOKEN, Some(&first), "heartbeat_list", json!({}))
        .await;
    let own = structured(&own)["heartbeats"].as_array().unwrap();
    assert_eq!(own.len(), 1);
    assert_eq!(own[0]["id"], heartbeat);
    for caller in [Some(second.as_str()), None] {
        let other = scoped
            .call(TOKEN, caller, "heartbeat_list", json!({}))
            .await;
        assert_eq!(structured(&other)["heartbeats"], json!([]));
    }

    let foreign = scoped
        .call(
            TOKEN,
            Some(&second),
            "heartbeat_delete",
            json!({"heartbeatId":heartbeat}),
        )
        .await;
    assert_eq!(text(&foreign), model::ErrorCode::InvalidMessage.message());
    structured(
        &scoped
            .call(
                TOKEN,
                Some(&first),
                "delete_heartbeat",
                json!({"id":heartbeat}),
            )
            .await,
    );
    let another = scoped
        .call(TOKEN, Some(&first), "create_heartbeat", create)
        .await;
    let another = structured(&another)["schedule"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    structured(
        &scoped
            .call(
                TOKEN,
                Some(&first),
                "heartbeat_delete",
                json!({"heartbeatId":another}),
            )
            .await,
    );
    let emptied = scoped
        .call(TOKEN, Some(&first), "heartbeat_list", json!({}))
        .await;
    assert_eq!(structured(&emptied)["heartbeats"], json!([]));
    scoped.stop().await;
}
