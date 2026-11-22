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

#[tokio::test]
async fn every_workspace_has_one_notepad_that_host_callers_share() {
    let scoped = Scoped::start().await;
    let agent = scoped.agent(json!({})).await;
    let root = scoped.workspace.clone();

    let unplaced = scoped.call(TOKEN, None, "notepad_read", json!({})).await;
    assert_eq!(text(&unplaced), model::ErrorCode::InvalidMessage.message());

    let empty = scoped
        .call(TOKEN, None, "notepad_read", json!({"workspaceId":root}))
        .await;
    assert_eq!(
        structured(&empty),
        &json!({"workspaceId":root,"revision":0,"text":"","chars":0,"nextOffset":null})
    );

    let external = scoped
        .call(
            TOKEN,
            None,
            "notepad_write",
            json!({"workspaceId":root,"text":"API: POST /x"}),
        )
        .await;
    let external = structured(&external);
    assert_eq!(
        (&external["accepted"], &external["revision"]),
        (&json!(true), &json!(1))
    );
    assert_eq!(external["updatedBy"], json!({"kind":"external"}));

    let question = scoped
        .call(
            AGENT_MCP_TOKEN,
            Some(&agent),
            "notepad_write",
            json!({"text":"Which port?","mode":"append"}),
        )
        .await;
    let question = structured(&question);
    assert_eq!(question["workspaceId"], root);
    assert_eq!(
        question["updatedBy"],
        json!({"kind":"agent","agentId":agent,"title":"Scoped Agent"})
    );

    let read = scoped
        .call(TOKEN, Some(&agent), "notepad_read", json!({}))
        .await;
    let read = structured(&read);
    let shared = read["text"].as_str().unwrap();
    assert!(
        shared.starts_with("API: POST /x\n\n[Scoped Agent · "),
        "{shared}"
    );
    assert!(shared.ends_with("]\nWhich port?"));
    assert_eq!(
        (&read["revision"], &read["nextOffset"]),
        (&json!(2), &Value::Null)
    );

    // What a client writes without an author is a user's; this is what the Notepad tab does.
    let by_user = scoped
        .rpc(
            "workspace.notepad.write.request",
            json!({"workspaceId":root,"text":"cleared","expected":2}),
        )
        .await;
    assert_eq!(by_user["updatedBy"], json!({"kind":"user"}));

    let elsewhere = scoped
        .call(
            TOKEN,
            None,
            "notepad_read",
            json!({"workspaceId":scoped.other}),
        )
        .await;
    assert_eq!(structured(&elsewhere)["revision"], 0);
    let missing = scoped
        .call(
            TOKEN,
            None,
            "notepad_read",
            json!({"workspaceId":"wks_missing"}),
        )
        .await;
    assert_eq!(
        text(&missing),
        model::ErrorCode::WorkspaceNotFound.message()
    );
    scoped.stop().await;
}

#[tokio::test]
async fn the_notepad_is_read_in_pages_and_replaced_only_with_the_revision_read() {
    let scoped = Scoped::start().await;
    let root = scoped.workspace.clone();
    let call = async |tool: &str, mut arguments: Value| {
        arguments["workspaceId"] = json!(root);
        scoped.call(TOKEN, None, tool, arguments).await
    };
    let long = format!("{}{}", "a".repeat(4000), "é".repeat(1000));
    structured(&call("notepad_write", json!({"text":long,"expected":0})).await);

    let first = call("notepad_read", json!({})).await;
    let first = structured(&first);
    assert_eq!(
        (&first["chars"], &first["nextOffset"]),
        (&json!(5000), &json!(4000))
    );
    assert_eq!(first["text"].as_str().unwrap().len(), 4000);
    let rest = call("notepad_read", json!({"offset":4000,"limit":8000})).await;
    let rest = structured(&rest);
    assert_eq!(rest["text"].as_str().unwrap().chars().count(), 1000);
    assert_eq!(rest["nextOffset"], Value::Null);
    let beyond = call("notepad_read", json!({"offset":9000})).await;
    assert_eq!(structured(&beyond)["text"], "");

    for stale in [
        json!({"text":"short"}),
        json!({"text":"short","expected":4}),
    ] {
        let refused_write = call("notepad_write", stale).await;
        let refusal = refused(&refused_write);
        assert_eq!(
            (&refusal["accepted"], &refusal["revision"]),
            (&json!(false), &json!(1))
        );
        assert_eq!(refusal["updatedBy"], json!({"kind":"external"}));
        assert!(refusal["error"].as_str().unwrap().contains("read it again"));
    }
    let replaced = call("notepad_write", json!({"text":"short","expected":1})).await;
    assert_eq!(structured(&replaced)["revision"], 2);

    let invalid = call("notepad_write", json!({"text":"","mode":"append"})).await;
    assert_eq!(text(&invalid), model::ErrorCode::InvalidMessage.message());
    for (arguments, tool) in [
        (json!({"text":"t","mode":"merge"}), "notepad_write"),
        (json!({"key":"plan"}), "notepad_read"),
    ] {
        rejected(
            &call(tool, arguments).await,
            "Arguments do not match the tool input schema",
        );
    }
    rejected(
        &call("notepad_list", json!({})).await,
        "Unknown or unavailable tool",
    );
    scoped.stop().await;
}

#[tokio::test]
async fn a_bot_and_its_agents_use_the_notepad_of_their_workspaces_without_a_grant() {
    let scoped = Scoped::start().await;
    // No group is needed: the notepad belongs to the Workspace.
    scoped
        .setup(json!({"groups":["observe"],"modes":{"codex":["auto"]}}))
        .await;
    let owner = scoped.owner_credential().await;
    let child = scoped.subtree_agent().await;
    let subtree = scoped.subtree_credential(&child);
    let write = json!({"text":"step one","mode":"append"});

    for bearer in [owner.as_str(), subtree.as_str()] {
        let tools = scoped.tools(bearer).await;
        assert!(tools.contains("notepad_read") && tools.contains("notepad_write"));
    }
    let by_bot = scoped
        .call(&owner, None, "notepad_write", write.clone())
        .await;
    assert_eq!(structured(&by_bot)["workspaceId"], scoped.workspace);
    assert_eq!(structured(&by_bot)["updatedBy"], json!({"kind":"bot"}));
    let by_agent = scoped
        .call(&subtree, None, "notepad_write", write.clone())
        .await;
    assert_eq!(structured(&by_agent)["updatedBy"]["agentId"], child);
    let read = scoped.call(&subtree, None, "notepad_read", json!({})).await;
    let read = structured(&read);
    assert!(
        read["text"]
            .as_str()
            .unwrap()
            .starts_with("[Workspace Bot · ")
    );
    assert_eq!(read["revision"], 2);

    let overview = scoped.call(&owner, None, "bot_overview", json!({})).await;
    let notepad = &structured(&overview)["notepad"];
    assert_eq!(notepad["revision"], 2);
    assert_eq!(notepad["chars"], read["chars"]);

    let mut foreign = write.clone();
    foreign["workspaceId"] = json!(scoped.other);
    forbidden(&scoped.call(&subtree, None, "notepad_write", foreign).await);
    forbidden(
        &scoped
            .call(
                &owner,
                None,
                "notepad_read",
                json!({"workspaceId":scoped.other}),
            )
            .await,
    );
    scoped.stop().await;
}

#[tokio::test]
async fn a_conversation_restricted_to_its_own_work_may_not_use_the_shared_notepad() {
    let scoped = Scoped::start().await;
    scoped.setup(codex_modes(&["auto"])).await;
    let stranger = scoped.bot_credential();
    for (tool, arguments) in [
        ("notepad_read", json!({})),
        ("notepad_write", json!({"text":"t","mode":"append"})),
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
    let stored = scoped
        .rpc(
            "workspace.notepad.get.request",
            json!({"workspaceId":scoped.workspace}),
        )
        .await;
    assert_eq!(stored["revision"], 0);
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
