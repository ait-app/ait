//! `host_list` and calls routed with `hostId` to sibling Hosts: the calling Host's rules.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

use model::peer::{Error, Host, HostState, Peers, Request};
use provider::service::agent_mcp::GUEST_SHARE_LABEL;
use uuid::Uuid;

use super::support::{codex_modes, rejected, structured};
use super::*;

/// This daemon's `serverId` in these tests.
const LOCAL: &str = "6f1c2a4e-8b3d-4c5e-9a7f-1b2c3d4e5f60";
/// The text of a call whose reply was lost.
const OUTCOME_UNKNOWN: &str = "The call may have run on the other host. Check with agent_list \
    or agent_get before retrying.";
/// The text of a call refused while too many calls to other hosts are open.
const CAPACITY: &str = "Too many calls to other hosts are open right now; this one was not \
    sent. Retry later.";
/// The text of a request too large to send.
const REQUEST_TOO_LARGE: &str = "The request exceeds the 1 MiB message limit and was not sent. \
    Retrying unchanged fails the same way: send less.";
/// The text of a call that ran but whose reply was too large to return.
const REPLY_TOO_LARGE: &str = "The call ran on the other host, but its reply exceeds the 1 MiB \
    message limit. Do not repeat a call that changes something; ask for less (for example a \
    lower `limit`).";
/// The refusal of arguments outside the caller's schema.
const MISMATCH: &str = "Arguments do not match the tool input schema";

/// The account's Hosts behind a fake Hub link: a fixed Host list and one reply for every call,
/// recording each call it receives.
#[derive(Debug)]
struct Account {
    hosts: Result<Vec<Host>, Error>,
    reply: Mutex<Result<Value, Error>>,
    calls: Mutex<Vec<(Uuid, Request, Duration)>>,
}

impl Account {
    fn new(hosts: Result<Vec<Host>, Error>) -> Arc<Self> {
        Arc::new(Self {
            hosts,
            reply: Mutex::new(Err(Error::OutcomeUnknown)),
            calls: Mutex::new(Vec::new()),
        })
    }

    fn reply(&self, reply: Result<Value, Error>) {
        *self.reply.lock().unwrap() = reply;
    }

    fn calls(&self) -> Vec<(Uuid, Request, Duration)> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl Peers for Account {
    async fn hosts(&self) -> Result<Vec<Host>, Error> {
        self.hosts.clone()
    }

    async fn call(&self, node: Uuid, request: Request, timeout: Duration) -> Result<Value, Error> {
        self.calls.lock().unwrap().push((node, request, timeout));
        self.reply.lock().unwrap().clone()
    }
}

fn host(name: &str, state: HostState, supported: bool, enabled: bool) -> Host {
    Host {
        server_id: Uuid::new_v4(),
        host_id: Uuid::new_v4(),
        node_id: Uuid::new_v4(),
        name: name.to_owned(),
        hostname: format!("{name}.local"),
        platform: "linux".to_owned(),
        app_version: "1.2.3".to_owned(),
        state,
        supported,
        enabled,
    }
}

/// This Host as the center lists it: it does not accept calls itself.
fn this_host() -> Host {
    Host {
        server_id: LOCAL.parse().unwrap(),
        ..host("Laptop", HostState::Online, true, false)
    }
}

/// A Host that can be called now.
fn available() -> Host {
    host("Build", HostState::Online, true, true)
}

async fn start(account: &Arc<Account>) -> Scoped {
    let peers: Arc<dyn Peers> = account.clone();
    Scoped::start_as(LOCAL, Some(peers)).await
}

/// Assert that `response` is a failed tool result naming `host` with `code` and text `error`.
fn failed(response: &Value, host: &str, code: &str, error: &str) {
    let result = &response["result"];
    assert_eq!(result["isError"], true, "{response}");
    assert_eq!(result["structuredContent"]["hostId"], host, "{response}");
    assert_eq!(result["structuredContent"]["code"], code, "{response}");
    assert_eq!(result["structuredContent"]["error"], error, "{response}");
    assert_eq!(result["content"][0]["text"], error, "{response}");
}

/// The `host_list` result for a host credential.
async fn host_list(scoped: &Scoped) -> Value {
    let listed = scoped.call(TOKEN, None, "host_list", json!({})).await;
    structured(&listed).clone()
}

/// The `initialize` instructions `bearer` receives.
async fn instructions(scoped: &Scoped, bearer: &str) -> String {
    let initialized: Value = scoped
        .post(
            bearer,
            None,
            "initialize",
            json!({"protocolVersion":"2025-11-25","capabilities":{},
                "clientInfo":{"name":"test","version":"1"}}),
        )
        .await
        .json()
        .await
        .unwrap();
    initialized["result"]["instructions"]
        .as_str()
        .unwrap_or_else(|| panic!("{initialized}"))
        .to_owned()
}

/// The descriptor of each tool listed to `bearer`, by name.
async fn descriptors(scoped: &Scoped, bearer: &str) -> BTreeMap<String, Value> {
    let listed: Value = scoped
        .post(bearer, None, "tools/list", json!({}))
        .await
        .json()
        .await
        .unwrap();
    listed["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("{listed}"))
        .iter()
        .map(|tool| (tool["name"].as_str().unwrap().to_owned(), tool.clone()))
        .collect()
}

#[tokio::test]
async fn host_list_puts_this_host_first_and_explains_unavailable_ones() {
    let build = available();
    let account = Account::new(Ok(vec![
        build.clone(),
        this_host(),
        host("Closed", HostState::Online, true, false),
        host("Away", HostState::Offline, true, true),
        host("Resuming", HostState::Reconnecting, true, true),
        host("Old", HostState::Online, false, true),
    ]));
    let scoped = start(&account).await;

    let listed = host_list(&scoped).await;

    assert_eq!(listed["accepting"], false, "{listed}");
    assert!(listed.get("note").is_none(), "{listed}");
    let hosts = listed["hosts"].as_array().unwrap();
    let summary: Vec<_> = hosts
        .iter()
        .map(|host| {
            (
                host["name"].as_str().unwrap(),
                host["self"] == true,
                host["available"] == true,
                host.get("reason").and_then(Value::as_str),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("Laptop", true, true, None),
            ("Build", false, true, None),
            ("Closed", false, false, Some("not_accepting")),
            ("Away", false, false, Some("offline")),
            ("Resuming", false, false, Some("offline")),
            ("Old", false, false, Some("unsupported")),
        ]
    );
    assert_eq!(hosts[0]["hostId"], LOCAL);
    assert_eq!(
        hosts[1],
        json!({"hostId":build.server_id.to_string(),"name":"Build","hostname":"Build.local",
            "platform":"linux","appVersion":"1.2.3","state":"online","self":false,
            "available":true})
    );
    assert_eq!(hosts[4]["state"], "reconnecting");
    assert!(account.calls().is_empty());

    // `accepting` follows this Host's own switch.
    scoped
        .rpc(
            "daemon.config.set.request",
            json!({"config":{"mcp":{"acceptPeers":true}}}),
        )
        .await;
    assert_eq!(host_list(&scoped).await["accepting"], true);

    let tools = descriptors(&scoped, TOKEN).await;
    let listing = &tools["host_list"];
    assert_eq!(listing["annotations"]["readOnlyHint"], true, "{listing}");
    assert_eq!(
        listing["inputSchema"],
        json!({"type":"object","properties":{},"required":[],"additionalProperties":false})
    );
    scoped.stop().await;
}

#[tokio::test]
async fn without_a_hub_link_only_this_host_is_listed() {
    let account = Account::new(Err(Error::Unavailable));
    let scoped = start(&account).await;

    let listed = host_list(&scoped).await;

    let hosts = listed["hosts"].as_array().unwrap();
    assert_eq!(hosts.len(), 1, "{listed}");
    assert_eq!(hosts[0]["hostId"], LOCAL);
    assert_eq!(hosts[0]["self"], true);
    assert_eq!(hosts[0]["available"], true);
    assert_eq!(hosts[0]["appVersion"], model::BUILD_VERSION);
    assert!(
        listed["note"].as_str().is_some_and(|note| !note.is_empty()),
        "{listed}"
    );
    // Another Host cannot be resolved without the list.
    let other = Uuid::new_v4().to_string();
    let routed = scoped
        .call(
            TOKEN,
            None,
            "agent_get",
            json!({"agentId":"x","hostId":other}),
        )
        .await;
    failed(
        &routed,
        &other,
        "hub_unavailable",
        &Error::Unavailable.to_string(),
    );
    assert!(account.calls().is_empty());
    scoped.stop().await;

    // The daemon's own Hub link, never connected here, gives the same answer.
    let unlinked = Scoped::start().await;
    let listed = host_list(&unlinked).await;
    assert_eq!(listed["hosts"].as_array().unwrap().len(), 1, "{listed}");
    assert_eq!(listed["hosts"][0]["hostId"], "stable");
    assert!(listed["note"].is_string(), "{listed}");
    unlinked.stop().await;
}

#[tokio::test]
async fn a_remote_call_is_forwarded_without_host_id_and_tagged_on_return() {
    let target = available();
    let account = Account::new(Ok(vec![this_host(), target.clone()]));
    let remote = json!({"content":[{"type":"text","text":"{\"status\":\"idle\"}"}],
        "structuredContent":{"status":"idle"},"isError":false});
    account.reply(Ok(remote.clone()));
    let scoped = start(&account).await;
    let caller = scoped.agent(json!({})).await;
    let host = target.server_id.to_string();

    let response = scoped
        .call(
            TOKEN,
            Some(&caller),
            "agent_get",
            json!({"agentId":"x","hostId":host}),
        )
        .await;

    let mut tagged = remote;
    tagged["structuredContent"]["hostId"] = json!(host);
    assert_eq!(response["result"], tagged, "{response}");
    let calls = account.calls();
    let [(node, request, timeout)] = calls.as_slice() else {
        panic!("one call expected: {calls:?}");
    };
    assert_eq!(*node, target.node_id);
    assert_eq!(
        *request,
        Request {
            tool: "agent_get".to_owned(),
            arguments: json!({"agentId":"x"}),
            caller: Some(caller),
        }
    );
    // What remains of the 55 s the routed call started with (exact in `mcp::hosts::tests`).
    assert!(
        (Duration::from_secs(1)..=Duration::from_secs(55)).contains(timeout),
        "{timeout:?}"
    );

    // A retired name is forwarded under its current name, and the Host id is a UUID in any case;
    // without a caller header no caller is sent.
    let upper = host.to_uppercase();
    let archived = scoped
        .call(
            TOKEN,
            None,
            "kill_agent",
            json!({"agentId":"x","hostId":upper}),
        )
        .await;
    assert_eq!(archived["result"]["isError"], false, "{archived}");
    let (node, request, _) = account.calls().pop().unwrap();
    assert_eq!(node, target.node_id);
    assert_eq!(
        request,
        Request {
            tool: "agent_archive".to_owned(),
            arguments: json!({"agentId":"x"}),
            caller: None,
        }
    );
    scoped.stop().await;
}

#[tokio::test]
async fn the_local_host_id_and_no_host_id_take_the_same_path() {
    let account = Account::new(Ok(vec![this_host(), available()]));
    let scoped = start(&account).await;

    for tool in ["agent_list", "workspace_list"] {
        let plain = scoped.call(TOKEN, None, tool, json!({})).await;
        assert!(
            structured(&plain).get("hostId").is_none(),
            "{tool}: {plain}"
        );
        for local in [LOCAL.to_owned(), LOCAL.to_uppercase()] {
            let routed = scoped
                .call(TOKEN, None, tool, json!({"hostId":local}))
                .await;
            assert_eq!(routed, plain, "{tool} {local}");
        }
    }
    assert!(account.calls().is_empty());
    scoped.stop().await;
}

#[tokio::test]
async fn failures_name_the_host_and_never_replay() {
    let target = available();
    let closed = host("Closed", HostState::Online, true, false);
    let away = host("Away", HostState::Offline, true, true);
    let old = host("Old", HostState::Online, false, true);
    let account = Account::new(Ok(vec![
        this_host(),
        target.clone(),
        closed.clone(),
        away.clone(),
        old.clone(),
    ]));
    let scoped = start(&account).await;
    let host = target.server_id.to_string();

    // The reply to an `agent_create` was lost: the outcome is unknown and it is not sent again.
    account.reply(Err(Error::OutcomeUnknown));
    let lost = scoped
        .call(
            TOKEN,
            None,
            "agent_create",
            json!({"hostId":host,"workspaceId":"wks_remote","provider":"codex/offline-model",
                "title":"Remote","initialPrompt":"work","idempotencyKey":"create-1"}),
        )
        .await;
    failed(&lost, &host, "outcome_unknown", OUTCOME_UNKNOWN);
    let calls = account.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1.tool, "agent_create");
    assert_eq!(calls[0].1.arguments["idempotencyKey"], "create-1");

    // Every other way a sent call fails names the host and its reason.
    let mut sent = calls.len();
    // The capacity cases each say whether the call ran and what to do; the rest name the error.
    for (error, code, text) in [
        (Error::Unavailable, "hub_unavailable", None),
        (Error::UnknownHost, "host_unknown", None),
        (Error::Offline, "host_offline", None),
        (Error::NotAccepting, "host_not_accepting", None),
        (Error::Unsupported, "host_unsupported", None),
        (Error::Capacity, "capacity", Some(CAPACITY)),
        (Error::RequestTooLarge, "capacity", Some(REQUEST_TOO_LARGE)),
        (Error::ReplyTooLarge, "capacity", Some(REPLY_TOO_LARGE)),
    ] {
        account.reply(Err(error.clone()));
        let response = scoped
            .call(
                TOKEN,
                None,
                "agent_get",
                json!({"agentId":"x","hostId":host}),
            )
            .await;
        failed(&response, &host, code, text.unwrap_or(&error.to_string()));
        sent += 1;
        assert_eq!(account.calls().len(), sent, "{error:?}");
    }
    let message = "Unknown or unavailable tool";
    account.reply(Err(Error::Rejected {
        code: -32602,
        message: message.to_owned(),
    }));
    let rejected_call = scoped
        .call(
            TOKEN,
            None,
            "agent_get",
            json!({"agentId":"x","hostId":host}),
        )
        .await;
    failed(&rejected_call, &host, "host_rejected", message);
    assert_eq!(
        rejected_call["result"]["structuredContent"]["remoteCode"],
        -32602
    );
    sent += 1;

    // Hosts that cannot take the call now, or are not on the account, are never called.
    let unknown = Uuid::new_v4().to_string();
    for (target, code, error) in [
        (unknown.as_str(), "host_unknown", Error::UnknownHost),
        ("not-a-host-id", "host_unknown", Error::UnknownHost),
        (
            &closed.server_id.to_string(),
            "host_not_accepting",
            Error::NotAccepting,
        ),
        (&away.server_id.to_string(), "host_offline", Error::Offline),
        (
            &old.server_id.to_string(),
            "host_unsupported",
            Error::Unsupported,
        ),
    ] {
        let response = scoped
            .call(
                TOKEN,
                None,
                "agent_get",
                json!({"agentId":"x","hostId":target}),
            )
            .await;
        failed(&response, target, code, &error.to_string());
    }
    assert_eq!(account.calls().len(), sent);
    scoped.stop().await;
}

/// An Agent of the `other` Workspace once it is archived, which `workspace.get` no longer
/// describes; such a caller counts as sandboxed.
async fn agent_of_an_archived_workspace(scoped: &Scoped) -> String {
    let agent = scoped
        .agent_in(&scoped.other, &scoped.other_cwd, json!({}), None)
        .await;
    let archived = scoped
        .call(
            TOKEN,
            None,
            "workspace_archive",
            json!({"workspaceId":scoped.other}),
        )
        .await;
    assert_eq!(archived["result"]["isError"], false, "{archived}");
    let described = scoped
        .rpc("workspace.get.request", json!({"workspaceId":scoped.other}))
        .await;
    assert_eq!(described["state"], "archived", "{described}");
    assert!(described["workspace"].is_null(), "{described}");
    agent
}

#[tokio::test]
async fn scoped_sandboxed_and_guest_callers_cannot_route() {
    let target = available();
    let account = Account::new(Ok(vec![this_host(), target.clone()]));
    let scoped = start(&account).await;
    scoped.setup(Value::Null).await;
    let host = target.server_id.to_string();
    let subtree = scoped.subtree_agent().await;

    let host_instructions = instructions(&scoped, TOKEN).await;
    assert!(
        host_instructions.ends_with(
            " Use host_list to see other hosts on this account; pass hostId to run agent, \
            workspace and provider tools there."
        ),
        "{host_instructions}"
    );
    for credential in [scoped.bot_credential(), scoped.subtree_credential(&subtree)] {
        let scoped_instructions = instructions(&scoped, &credential).await;
        assert!(
            host_instructions.starts_with(&scoped_instructions),
            "{scoped_instructions}"
        );
        assert!(
            !scoped_instructions.contains("host"),
            "{scoped_instructions}"
        );
        let tools = descriptors(&scoped, &credential).await;
        assert!(!tools.contains_key("host_list"), "{tools:?}");
        assert!(tools.contains_key("agent_get"), "{tools:?}");
        for (name, tool) in &tools {
            assert!(
                tool["inputSchema"]["properties"].get("hostId").is_none(),
                "{name}"
            );
        }
        rejected(
            &scoped
                .call(
                    &credential,
                    None,
                    "agent_get",
                    json!({"agentId":subtree,"hostId":host}),
                )
                .await,
            MISMATCH,
        );
        rejected(
            &scoped.call(&credential, None, "host_list", json!({})).await,
            "Unknown or unavailable tool",
        );
    }

    // Host credentials acting for an Agent in a sandboxed worktree, or for a guest's Agent.
    let created = scoped
        .call(
            TOKEN,
            None,
            "workspace_create",
            json!({"isolation":"sandboxed-worktree","path":scoped.cwd}),
        )
        .await;
    let workspace = &structured(&created)["workspace"];
    assert_eq!(workspace["sandbox"], "worktree_writes", "{created}");
    let sandboxed = scoped
        .agent_in(
            workspace["id"].as_str().unwrap(),
            std::path::Path::new(workspace["workspaceDirectory"].as_str().unwrap()),
            json!({}),
            None,
        )
        .await;
    let guest = scoped
        .agent(json!({GUEST_SHARE_LABEL:Uuid::new_v4().to_string()}))
        .await;
    let unplaced = agent_of_an_archived_workspace(&scoped).await;
    for caller in [sandboxed, guest, unplaced] {
        let response = scoped
            .call(
                TOKEN,
                Some(&caller),
                "agent_get",
                json!({"agentId":"x","hostId":host}),
            )
            .await;
        let result = &response["result"];
        assert_eq!(result["isError"], true, "{response}");
        assert_eq!(result["structuredContent"]["code"], "caller_not_allowed");
        assert_eq!(result["structuredContent"]["hostId"], host);
    }
    assert!(account.calls().is_empty());
    scoped.stop().await;
}

#[tokio::test]
async fn bot_scoped_agents_and_ticket_workers_cannot_route() {
    let target = available();
    let account = Account::new(Ok(vec![this_host(), target.clone()]));
    let scoped = start(&account).await;
    scoped.setup(codex_modes(&["auto"])).await;
    let host = target.server_id.to_string();
    let bot = scoped.bot_credential_in(scoped.conversation().await);
    let created = scoped
        .call(
            &bot,
            None,
            "agent_create",
            json!({"provider":"codex/offline-model","title":"Fix login","initialPrompt":"work"}),
        )
        .await;
    let worker_agent = structured(&created)["agent"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let worker = scoped.session_credential(&worker_agent).await;

    let tools = descriptors(&scoped, &worker).await;
    assert!(tools.contains_key("ticket_report"), "{tools:?}");
    assert!(!tools.contains_key("host_list"), "{tools:?}");
    for (name, tool) in &tools {
        assert!(
            tool["inputSchema"]["properties"].get("hostId").is_none(),
            "{name}"
        );
    }
    rejected(
        &scoped
            .call(&worker, None, "agent_list", json!({"hostId":host}))
            .await,
        MISMATCH,
    );
    rejected(
        &scoped.call(&worker, None, "host_list", json!({})).await,
        "Unknown or unavailable tool",
    );

    // Host credentials acting for a Bot-scoped Agent, a ticket worker or a subtree Agent, are
    // refused as they are for local calls.
    let subtree = scoped.subtree_agent().await;
    for caller in [worker_agent, subtree] {
        rejected(
            &scoped
                .call(
                    TOKEN,
                    Some(&caller),
                    "agent_get",
                    json!({"agentId":"x","hostId":host}),
                )
                .await,
            "Bot-scoped agents must use their own credential",
        );
    }
    assert!(account.calls().is_empty());
    scoped.stop().await;
}

#[tokio::test]
async fn tools_without_the_peer_mark_refuse_host_id() {
    let target = available();
    let account = Account::new(Ok(vec![this_host(), target.clone()]));
    let scoped = start(&account).await;
    let host = target.server_id.to_string();

    rejected(
        &scoped
            .call(TOKEN, None, "terminal_list", json!({"hostId":host}))
            .await,
        MISMATCH,
    );
    rejected(
        &scoped
            .call(TOKEN, None, "agent_list", json!({"hostId":""}))
            .await,
        MISMATCH,
    );

    // Host credentials see `hostId` on exactly the tools marked for peers.
    let routable: BTreeSet<_> = descriptors(&scoped, TOKEN)
        .await
        .into_iter()
        .filter(|(_, tool)| {
            tool["inputSchema"]["properties"]["hostId"] == json!({"type":"string","minLength":1})
        })
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        routable,
        BTreeSet::from(
            [
                "agent_create",
                "agent_prompt",
                "agent_get",
                "agent_activity_read",
                "agent_list",
                "agent_wait",
                "agent_cancel",
                "agent_archive",
                "workspace_create",
                "workspace_list",
                "provider_list",
                "provider_model_list",
                "profile_list",
            ]
            .map(str::to_owned)
        )
    );
    assert!(account.calls().is_empty());
    scoped.stop().await;
}
