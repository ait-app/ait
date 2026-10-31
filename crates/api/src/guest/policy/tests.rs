use serde_json::json;

use super::*;
use crate::guest::admit;
use crate::test_support::{Workspaces, address};

mod scopes;

struct Guest {
    fixture: Workspaces,
    session: Session,
}

impl Guest {
    async fn new() -> Self {
        Self::with(ResourceKind::Workspace).await
    }

    /// A guest of a share of the fixture's shared workspace, shared project, or node.
    async fn with(kind: ResourceKind) -> Self {
        let fixture = Workspaces::new(address());
        let key = match kind {
            ResourceKind::Workspace => fixture.shared.clone(),
            ResourceKind::Project => fixture.project.clone(),
            ResourceKind::Node => String::new(),
        };
        let visitor = fixture.accept(kind, &key).await;
        let session = admit(&fixture.api.shared, &visitor.encode()).await.unwrap();
        Self { fixture, session }
    }

    async fn private(&self, method: &str, params: Value) -> Value {
        let decision = self.decide(method, params).await;
        let Decision::Private(request, private) = decision else {
            panic!("{method} was not private: {decision:?}");
        };
        private::execute(
            &self.session,
            &self.fixture.api.shared,
            method,
            request.params,
            private,
        )
        .await
        .unwrap()
    }

    fn path(&self, name: &str) -> String {
        self.fixture.path(name).to_string_lossy().into_owned()
    }

    async fn decide(&self, method: &str, params: Value) -> Decision {
        let incoming = Request {
            id: "r".into(),
            method: method.into(),
            params,
        };
        request(&self.session, &self.fixture.api.shared, incoming).await
    }

    async fn refused(&self, method: &str, params: Value) -> ErrorCode {
        match self.decide(method, params).await {
            Decision::Refuse(_, code) => code,
            decision => panic!("{method} was not refused: {decision:?}"),
        }
    }

    async fn forwarded(&self, method: &str, params: Value) -> Value {
        match self.decide(method, params).await {
            Decision::Forward(request) => request.params,
            decision => panic!("{method} was not forwarded: {decision:?}"),
        }
    }

    fn root(&self) -> String {
        self.fixture.shared_path().to_string_lossy().into_owned()
    }

    fn outside(&self) -> String {
        self.fixture
            .root
            .path()
            .join("other")
            .to_string_lossy()
            .into_owned()
    }
}

#[test]
fn methods_are_classified_by_default_deny() {
    for (method, rule) in [
        ("server.info", Rule::Open),
        ("subscription.release.request", Rule::Open),
        ("checkout.diff.unsubscribe.request", Rule::Open),
        ("provider.models.list.request", Rule::Catalog),
        ("provider.snapshot.refresh.request", Rule::Deny),
        ("workspace.mark_unread.request", Rule::Workspace),
        ("workspace.list.request", Rule::WorkspaceList),
        ("agent.get.request", Rule::Agent("agentId")),
        ("agent.timeline.get.request", Rule::Agent("agentId")),
        ("agent.message.send.request", Rule::AgentDrive("agentId")),
        (
            "agent.permission.resolve.request",
            Rule::AgentDrive("agentId"),
        ),
        ("agent.cancel.request", Rule::AgentDrive("agentId")),
        (
            "agent.provider_subagents.list.request",
            Rule::Agent("parentAgentId"),
        ),
        ("agent.history.get.request", Rule::AgentFilter),
        ("agent.items.close.request", Rule::ItemsClose),
        ("terminal.input", Rule::Deny),
        ("terminal.kill.request", Rule::Terminal),
        ("checkout.push.request", Rule::Path),
        ("fs.file.download_token.request", Rule::Download),
    ] {
        assert_eq!(classify(method), rule, "{method}");
    }
    for denied in [
        "daemon.get_status.request",
        "daemon.config.set.request",
        "daemon.update.request",
        "server.shutdown.request",
        "server.restart.request",
        "diagnostics.request",
        "push.unregister.request",
        "relay.start.request",
        "publication.status.request",
        "schedule.create.request",
        "bot.create.request",
        "browser.tunnel.open.request",
        "voice.mode.set.request",
        "dictation.stream.start",
        "session.events.set_subscription.request",
        "creation.subscribe.request",
        "editor.open.request",
        "share.list.request",
        "share.set.request",
        "share.groups.list.request",
        "agent.configure",
        "agent.list",
        "agent.default.set",
        "agent.config.apply.request",
        "agent.skills.reconcile.request",
        "agent.import.request",
        "agent.refresh.request",
        "agent.resume.request",
        "agent.fork_context.request",
        "agent.delete.request",
        "agent.detach.request",
        "agent.timeline.append.request",
        "provider.usage.list.request",
        "provider.diagnostic.request",
        "provider.sessions.recent.list.request",
        "file.upload.request",
        "unknown.method",
    ] {
        assert_eq!(classify(denied), Rule::Deny, "{denied}");
    }
    for (method, rule) in [
        ("workspace.create.request", Rule::WorkspaceCreate),
        ("workspace.archive.request", Rule::WorkspaceManage),
        ("workspace.worktree.create.request", Rule::WorktreeCreate),
        ("workspace.worktree.archive.request", Rule::WorktreeArchive),
        (
            "workspace.worktree.list.request",
            Rule::Paths(&["cwd", "repoRoot"]),
        ),
        ("workspace.open.request", Rule::WorkspaceOpen),
        ("project.list.request", Rule::ProjectList),
        ("project.icon.get.request", Rule::Project),
        ("project.add.request", Rule::Node),
        ("project.rename.request", Rule::Node),
        ("workspace.label.list.request", Rule::Node),
        ("workspace.recovery.inspect.request", Rule::Node),
        ("workspace.setup.run.request", Rule::Node),
    ] {
        assert_eq!(classify(method), rule, "{method}");
        assert!(!rule.admits(ResourceKind::Workspace), "{method}");
        assert!(rule.admits(ResourceKind::Node), "{method}");
    }
    assert!(!Rule::Node.admits(ResourceKind::Project));
    assert!(Rule::WorkspaceCreate.admits(ResourceKind::Project));
    assert!(!Rule::Deny.admits(ResourceKind::Node));
}

#[tokio::test]
async fn host_and_owner_methods_are_forbidden() {
    let guest = Guest::new().await;
    for method in [
        "daemon.get_status.request",
        "share.list.request",
        "provider.snapshot.refresh.request",
    ] {
        assert_eq!(guest.refused(method, json!({})).await, ErrorCode::Forbidden);
    }
    let ping = guest
        .forwarded("connection.ping", json!({"nonce": "n"}))
        .await;
    assert_eq!(ping, json!({"nonce": "n"}));
}

#[tokio::test]
async fn workspace_methods_name_only_the_shared_workspace() {
    let guest = Guest::new().await;
    let shared = guest.fixture.shared.clone();
    let other = guest.fixture.other.clone();
    guest
        .forwarded("workspace.get.request", json!({"workspaceId": shared}))
        .await;
    guest
        .forwarded(
            "workspace.clear_attention.request",
            json!({"workspaceId": [shared]}),
        )
        .await;
    for params in [
        json!({"workspaceId": other}),
        json!({"workspaceId": [shared, other]}),
        json!({"workspaceId": []}),
        json!({}),
    ] {
        assert_eq!(
            guest
                .refused("workspace.clear_attention.request", params)
                .await,
            ErrorCode::Forbidden
        );
    }
}

#[tokio::test]
async fn workspace_lists_are_private_and_filtered() {
    let guest = Guest::new().await;
    assert_eq!(
        guest
            .refused("workspace.list.request", json!({"subscribe": {}}))
            .await,
        ErrorCode::Forbidden
    );
    let decision = guest
        .decide(
            "workspace.list.request",
            json!({"page": {"limit": 1}, "sync": {}, "subscribe": null}),
        )
        .await;
    let Decision::Private(request, private) = decision else {
        panic!("workspace list was not private: {decision:?}");
    };
    assert_eq!(private, Private::WorkspaceList);
    assert_eq!(request.params, json!({"page": {"limit": 1}}));
    // The shared workspace is found wherever it sorts in the directory.
    for direction in ["asc", "desc"] {
        let params = json!({"sort": [{"key": "name", "direction": direction}],
            "page": {"limit": 1}});
        let Decision::Private(request, private) =
            guest.decide("workspace.list.request", params).await
        else {
            panic!("workspace list was not private");
        };
        let listed = private::execute(
            &guest.session,
            &guest.fixture.api.shared,
            "workspace.list.request",
            request.params,
            private,
        )
        .await
        .unwrap();
        let entries = listed["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1, "{direction}");
        assert_eq!(entries[0]["id"], json!(guest.fixture.shared));
        assert_eq!(listed["emptyProjects"], json!([]));
        assert_eq!(listed["pageInfo"]["hasMore"], false);
    }
}

#[test]
fn workspace_listings_keep_only_the_guest_workspace() {
    let listing = json!({
        "entries": [{"id": "a", "name": "A"}, {"id": "b"}],
        "emptyProjects": [{"projectId": "p"}],
        "pageInfo": {"nextCursor": "c", "prevCursor": null, "hasMore": true},
        "sync": {"cursor": "x"},
    });
    assert_eq!(
        private::filter_workspaces(&listing, private::Keep::Workspace("a")),
        json!({
            "entries": [{"id": "a", "name": "A"}],
            "emptyProjects": [],
            "pageInfo": {"nextCursor": null, "prevCursor": null, "hasMore": false},
        })
    );
    assert_eq!(
        private::filter_workspaces(&json!({}), private::Keep::Workspace("a"))["entries"],
        json!([])
    );
}

#[tokio::test]
async fn paths_must_stay_inside_the_workspace() {
    let guest = Guest::new().await;
    let root = guest.root();
    guest
        .forwarded("fs.explorer.request", json!({"cwd": root}))
        .await;
    let escape = format!("{root}/../other");
    for params in [
        json!({"cwd": guest.outside()}),
        json!({"cwd": escape}),
        json!({"cwd": "relative"}),
        json!({}),
    ] {
        assert_eq!(
            guest.refused("checkout.status.get.request", params).await,
            ErrorCode::Forbidden
        );
    }
    assert_eq!(
        guest
            .refused("directory.suggestions.request", json!({"query": "/"}))
            .await,
        ErrorCode::Forbidden
    );
    guest
        .forwarded("provider.models.list.request", json!({"provider": "codex"}))
        .await;
    assert_eq!(
        guest
            .refused(
                "provider.models.list.request",
                json!({"provider": "codex", "cwd": guest.outside()}),
            )
            .await,
        ErrorCode::Forbidden
    );
}

#[tokio::test]
async fn download_tokens_are_rate_limited() {
    let guest = Guest::new().await;
    let params = json!({"cwd": guest.root(), "path": "file"});
    for _ in 0..16 {
        guest
            .forwarded("fs.file.download_token.request", params.clone())
            .await;
    }
    assert_eq!(
        guest
            .refused("fs.file.download_token.request", params)
            .await,
        ErrorCode::ResourceExhausted
    );
}

#[tokio::test]
async fn agent_creation_is_forced_into_the_workspace() {
    let guest = Guest::new().await;
    let created = guest
        .forwarded(
            "agent.create.request",
            json!({
                "config": {"provider": "codex", "cwd": guest.outside()},
                "workspaceId": guest.fixture.other,
                "labels": {"topic": "demo"},
            }),
        )
        .await;
    assert_eq!(created["workspaceId"], json!(guest.fixture.shared));
    assert_eq!(created["config"]["cwd"], json!(guest.root()));
    let grant = guest.session.registration.grant();
    assert_eq!(created["labels"]["topic"], "demo");
    assert_eq!(
        created["labels"][GUEST_SHARE],
        json!(grant.share_id.to_string())
    );
    assert_eq!(
        created["labels"][GUEST_USER],
        json!(grant.user_id.to_string())
    );
    for params in [
        json!({"config": {"provider": "codex"}, "env": {"A": "1"}}),
        json!({"config": {"provider": "codex"}, "worktree": {"mode": "new"}}),
        json!({"config": {"provider": "codex"}, "git": {}}),
        json!({"config": {"provider": "codex"}, "worktreeName": "x"}),
        json!({"config": {"provider": "codex", "mcpServers": {"x": {}}}}),
        json!({"config": {"provider": "codex", "providerOptions": {}}}),
        json!({"config": {"provider": "codex"}, "labels": {GUEST_SHARE: "forged"}}),
        json!({"config": {"provider": "codex"}, "callerAgentId": "owner-agent"}),
    ] {
        assert_eq!(
            guest.refused("agent.create.request", params).await,
            ErrorCode::Forbidden
        );
    }
    assert_eq!(
        guest.refused("agent.create.request", json!({})).await,
        ErrorCode::InvalidMessage
    );
}

const GUEST_SHARE: &str = provider::service::agent_mcp::GUEST_SHARE_LABEL;
const GUEST_USER: &str = provider::service::agent_mcp::GUEST_USER_LABEL;

#[tokio::test]
async fn agent_directories_are_filtered_and_unknown_agents_forbidden() {
    let guest = Guest::new().await;
    let workspaces = json!([guest.fixture.shared]);
    let listed = guest
        .forwarded(
            "agent.list.request",
            json!({"filter": {"workspaceIds": [guest.fixture.other]}}),
        )
        .await;
    assert_eq!(listed["filter"]["workspaceIds"], workspaces);
    let history = guest
        .forwarded("agent.history.get.request", json!(null))
        .await;
    assert_eq!(history["filter"]["workspaceIds"], workspaces);
    for (method, params) in [
        (
            "agent.message.send.request",
            json!({"agentId": "a", "text": "hi"}),
        ),
        (
            "agent.update.request",
            json!({"agentId": "a", "labels": {}}),
        ),
        ("agent.commands.list.request", json!({"draftConfig": {}})),
        (
            "agent.timeline.set_subscription.request",
            json!({"agentIds": ["a"]}),
        ),
        ("agent.items.close.request", json!({"terminalIds": ["t"]})),
        ("agent.cancel.request", json!({})),
    ] {
        assert_eq!(guest.refused(method, params).await, ErrorCode::Forbidden);
    }
    guest
        .forwarded(
            "agent.timeline.set_subscription.request",
            json!({"agentIds": []}),
        )
        .await;
}

#[tokio::test]
async fn terminals_are_forced_into_the_workspace() {
    let guest = Guest::new().await;
    let decision = guest
        .decide("terminal.create.request", json!({"cwd": guest.root()}))
        .await;
    let Decision::Private(request, Private::TerminalCreate) = decision else {
        panic!("terminal creation was not private: {decision:?}");
    };
    assert_eq!(request.params["workspaceId"], json!(guest.fixture.shared));
    let defaulted = guest.decide("terminal.create.request", json!({})).await;
    let Decision::Private(request, _) = defaulted else {
        panic!("terminal creation was not private: {defaulted:?}");
    };
    assert_eq!(request.params["cwd"], json!(guest.root()));
    assert_eq!(
        guest
            .refused("terminal.create.request", json!({"cwd": guest.outside()}))
            .await,
        ErrorCode::Forbidden
    );
    let listed = guest
        .forwarded(
            "terminal.list.subscribe.request",
            json!({"workspaceId": guest.fixture.other}),
        )
        .await;
    assert_eq!(listed["workspaceId"], json!(guest.fixture.shared));
    assert_eq!(listed["cwd"], json!(guest.root()));
    assert_eq!(
        guest
            .refused("terminal.kill.request", json!({"terminalId": "t"}))
            .await,
        ErrorCode::Forbidden
    );
}

#[tokio::test]
async fn events_are_confined() {
    let guest = Guest::new().await;
    let state = &guest.fixture.api.shared;
    let session = &guest.session;
    assert_eq!(
        event(session, state, "session.heartbeat", &json!({})).await,
        EventDecision::Drop
    );
    assert_eq!(
        event(session, state, "push.register", &json!({})).await,
        EventDecision::Refuse(ErrorCode::Forbidden)
    );
    assert_eq!(
        event(
            session,
            state,
            "terminal.input",
            &json!({"terminalId": "t"})
        )
        .await,
        EventDecision::Refuse(ErrorCode::Forbidden)
    );
    assert_eq!(
        event(session, state, "terminal.input", &json!({})).await,
        EventDecision::Refuse(ErrorCode::Forbidden)
    );
    session.remember_terminal("known");
    assert_eq!(
        event(
            session,
            state,
            "terminal.input",
            &json!({"terminalId": "known"})
        )
        .await,
        EventDecision::Forward
    );
}

#[test]
fn only_agents_created_by_the_same_visitor_of_the_share_can_be_instructed() {
    let share = "0193f2c4-0000-7000-8000-000000000001";
    let guest = json!({"labels": {GUEST_SHARE: share, GUEST_USER: "u"}});
    let other_visitor = json!({"labels": {GUEST_SHARE: share, GUEST_USER: "v"}});
    let other_share = json!({"labels": {
        GUEST_SHARE: "0193f2c4-0000-7000-8000-000000000002", GUEST_USER: "u"}});
    let owner = json!({"labels": {}});
    assert!(agents::created_under(&guest, share, "u"));
    assert!(!agents::created_under(&other_visitor, share, "u"));
    assert!(!agents::created_under(&other_share, share, "u"));
    assert!(!agents::created_under(&owner, share, "u"));
    assert!(!agents::created_under(&json!({}), share, "u"));
}
