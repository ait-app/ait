//! Project and node shares: what each scope reaches beyond a single workspace.

use super::*;

#[tokio::test]
async fn workspace_shares_refuse_project_and_node_methods() {
    let guest = Guest::new().await;
    for (method, params) in [
        ("workspace.create.request", json!({})),
        (
            "workspace.archive.request",
            json!({"workspaceId": guest.fixture.shared}),
        ),
        (
            "workspace.worktree.create.request",
            json!({"cwd": guest.root()}),
        ),
        ("workspace.open.request", json!({"cwd": guest.root()})),
        ("project.list.request", json!({})),
        ("project.add.request", json!({"cwd": guest.root()})),
    ] {
        assert_eq!(
            guest.refused(method, params).await,
            ErrorCode::Forbidden,
            "{method}"
        );
    }
}

#[tokio::test]
async fn project_shares_reach_only_workspaces_of_the_project() {
    let guest = Guest::with(ResourceKind::Project).await;
    let fixture = &guest.fixture;
    for workspace in [&fixture.shared, &fixture.sibling] {
        guest
            .forwarded("workspace.get.request", json!({"workspaceId": workspace}))
            .await;
        guest
            .forwarded(
                "workspace.title.set.request",
                json!({"workspaceId": workspace, "title": "t"}),
            )
            .await;
    }
    guest
        .forwarded(
            "workspace.clear_attention.request",
            json!({"workspaceId": [fixture.shared, fixture.sibling]}),
        )
        .await;
    for params in [
        json!({"workspaceId": fixture.other}),
        json!({"workspaceId": [fixture.shared, fixture.other]}),
        json!({"workspaceId": "missing"}),
    ] {
        assert_eq!(
            guest.refused("workspace.get.request", params).await,
            ErrorCode::Forbidden
        );
    }
    assert_eq!(
        guest
            .refused(
                "workspace.archive.request",
                json!({"workspaceId": fixture.other}),
            )
            .await,
        ErrorCode::Forbidden
    );
}

#[tokio::test]
async fn project_paths_cover_the_root_and_the_project_workspaces() {
    let guest = Guest::with(ResourceKind::Project).await;
    for cwd in [guest.root(), guest.path("sibling")] {
        guest
            .forwarded("fs.explorer.request", json!({"cwd": cwd}))
            .await;
        guest
            .forwarded("checkout.status.get.request", json!({"cwd": cwd}))
            .await;
    }
    for cwd in [
        guest.outside(),
        guest.path("registry"),
        "relative".to_owned(),
    ] {
        assert_eq!(
            guest
                .refused("fs.explorer.request", json!({"cwd": cwd}))
                .await,
            ErrorCode::Forbidden
        );
    }
    guest
        .forwarded(
            "workspace.worktree.list.request",
            json!({"repoRoot": guest.root()}),
        )
        .await;
    for params in [
        json!({}),
        json!({"repoRoot": guest.root(), "cwd": guest.outside()}),
    ] {
        assert_eq!(
            guest
                .refused("workspace.worktree.list.request", params)
                .await,
            ErrorCode::Forbidden
        );
    }
}

#[tokio::test]
async fn project_shares_create_workspaces_and_worktrees_only_in_the_project() {
    let guest = Guest::with(ResourceKind::Project).await;
    let project = json!(guest.fixture.project);
    let created = guest
        .forwarded(
            "workspace.create.request",
            json!({"source": {"kind": "directory", "path": guest.root(),
                "projectId": guest.fixture.other_project}}),
        )
        .await;
    assert_eq!(created["source"]["projectId"], project);
    let worktree = guest
        .forwarded(
            "workspace.create.request",
            json!({"source": {"kind": "worktree"}}),
        )
        .await;
    assert_eq!(worktree["source"]["projectId"], project);
    let forced = guest
        .forwarded(
            "workspace.worktree.create.request",
            json!({"cwd": guest.path("sibling"), "projectId": guest.fixture.other_project}),
        )
        .await;
    assert_eq!(forced["projectId"], project);
    for (method, params, code) in [
        (
            "workspace.create.request",
            json!({"source": {"kind": "directory", "path": guest.outside()}}),
            ErrorCode::Forbidden,
        ),
        (
            "workspace.create.request",
            json!({"source": {"kind": "worktree", "cwd": guest.outside()}}),
            ErrorCode::Forbidden,
        ),
        (
            "workspace.create.request",
            json!({"source": {"kind": "directory", "path": guest.root()},
                "agent": {"provider": "codex"}}),
            ErrorCode::Forbidden,
        ),
        (
            "workspace.create.request",
            json!({"source": {"kind": "directory"}}),
            ErrorCode::InvalidMessage,
        ),
        (
            "workspace.create.request",
            json!({"source": {"kind": "elsewhere"}}),
            ErrorCode::InvalidMessage,
        ),
        (
            "workspace.worktree.create.request",
            json!({"cwd": guest.outside()}),
            ErrorCode::Forbidden,
        ),
        (
            "workspace.worktree.create.request",
            json!({}),
            ErrorCode::Forbidden,
        ),
    ] {
        assert_eq!(guest.refused(method, params).await, code, "{method}");
    }
}

#[tokio::test]
async fn project_shares_open_and_archive_only_project_workspaces() {
    let guest = Guest::with(ResourceKind::Project).await;
    guest
        .forwarded(
            "workspace.open.request",
            json!({"cwd": guest.path("sibling")}),
        )
        .await;
    std::fs::create_dir(guest.fixture.shared_path().join("nested")).unwrap();
    let nested = format!("{}/nested", guest.root());
    for cwd in [nested, guest.outside()] {
        assert_eq!(
            guest
                .refused("workspace.open.request", json!({"cwd": cwd}))
                .await,
            ErrorCode::Forbidden
        );
    }
    guest
        .forwarded(
            "workspace.worktree.archive.request",
            json!({"workspaceId": guest.fixture.sibling, "scope": "worktree"}),
        )
        .await;
    guest
        .forwarded(
            "workspace.worktree.archive.request",
            json!({"worktreePath": guest.path("sibling")}),
        )
        .await;
    for params in [
        json!({"workspaceId": guest.fixture.other}),
        json!({"workspaceId": guest.fixture.sibling, "repoRoot": guest.outside()}),
        json!({"scope": "worktree"}),
    ] {
        assert_eq!(
            guest
                .refused("workspace.worktree.archive.request", params)
                .await,
            ErrorCode::Forbidden
        );
    }
}

#[tokio::test]
async fn project_shares_cannot_manage_projects() {
    let guest = Guest::with(ResourceKind::Project).await;
    let project = guest.fixture.project.clone();
    for (method, params) in [
        ("project.rename.request", json!({"projectId": project})),
        ("project.remove.request", json!({"projectId": project})),
        (
            "project.config.write.request",
            json!({"repoRoot": guest.root()}),
        ),
        ("project.icon.set.request", json!({"projectId": project})),
        ("project.add.request", json!({"cwd": guest.root()})),
        (
            "project.icon.get.request",
            json!({"projectId": guest.fixture.other_project}),
        ),
        (
            "project.config.read.request",
            json!({"repoRoot": guest.outside()}),
        ),
        (
            "workspace.pin.set.request",
            json!({"workspaceId": guest.fixture.shared}),
        ),
        ("daemon.get_status.request", json!({})),
        ("share.list.request", json!({})),
    ] {
        assert_eq!(
            guest.refused(method, params).await,
            ErrorCode::Forbidden,
            "{method}"
        );
    }
    guest
        .forwarded("project.icon.get.request", json!({"projectId": project}))
        .await;
    guest
        .forwarded(
            "project.config.read.request",
            json!({"repoRoot": guest.root()}),
        )
        .await;
}

#[tokio::test]
async fn project_listings_keep_only_the_project() {
    let guest = Guest::with(ResourceKind::Project).await;
    let projects = guest
        .private("project.list.request", json!({"sync": {}}))
        .await;
    let projects = projects["projects"].as_array().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0]["projectId"], json!(guest.fixture.project));

    assert_eq!(
        guest
            .refused("workspace.list.request", json!({"subscribe": {}}))
            .await,
        ErrorCode::Forbidden
    );
    let decision = guest
        .decide(
            "workspace.list.request",
            json!({"filter": {"projectId": guest.fixture.other_project}, "page": {"limit": 1}}),
        )
        .await;
    let Decision::Private(request, Private::WorkspaceList) = decision else {
        panic!("workspace list was not private: {decision:?}");
    };
    assert_eq!(
        request.params,
        json!({"filter": {"projectId": guest.fixture.project}})
    );
    let listed = guest.private("workspace.list.request", json!({})).await;
    let mut ids: Vec<&str> = listed["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["id"].as_str().unwrap())
        .collect();
    ids.sort_unstable();
    let mut expected = vec![
        guest.fixture.shared.as_str(),
        guest.fixture.sibling.as_str(),
    ];
    expected.sort_unstable();
    assert_eq!(ids, expected);
    assert_eq!(listed["pageInfo"]["hasMore"], false);
}

#[test]
fn project_listings_filter_entries_and_empty_projects() {
    let listing = json!({
        "entries": [{"id": "a", "projectId": "p"}, {"id": "b", "projectId": "q"}],
        "emptyProjects": [{"projectId": "p"}, {"projectId": "q"}],
    });
    assert_eq!(
        private::filter_workspaces(&listing, private::Keep::Project("p")),
        json!({
            "entries": [{"id": "a", "projectId": "p"}],
            "emptyProjects": [{"projectId": "p"}],
            "pageInfo": {"nextCursor": null, "prevCursor": null, "hasMore": false},
        })
    );
    assert_eq!(
        private::filter_projects(&json!({"projects": [{"projectId": "q"}]}), "p"),
        json!({"projects": []})
    );
}

#[tokio::test]
async fn project_terminals_and_agents_need_a_project_workspace() {
    let guest = Guest::with(ResourceKind::Project).await;
    let sibling = guest.fixture.sibling.clone();
    let decision = guest
        .decide("terminal.create.request", json!({"workspaceId": sibling}))
        .await;
    let Decision::Private(request, Private::TerminalCreate) = decision else {
        panic!("terminal creation was not private: {decision:?}");
    };
    assert_eq!(request.params["workspaceId"], json!(sibling));
    assert_eq!(request.params["cwd"], json!(guest.path("sibling")));
    let listed = guest
        .forwarded(
            "terminal.list.request",
            json!({"workspaceId": guest.fixture.shared}),
        )
        .await;
    assert_eq!(listed["workspaceId"], json!(guest.fixture.shared));
    for (method, params) in [
        ("terminal.create.request", json!({"cwd": guest.root()})),
        (
            "terminal.create.request",
            json!({"workspaceId": guest.fixture.other}),
        ),
        (
            "terminal.create.request",
            json!({"workspaceId": sibling, "cwd": guest.outside()}),
        ),
        ("terminal.list.request", json!({})),
        ("terminal.kill.request", json!({"terminalId": "t"})),
    ] {
        assert_eq!(
            guest.refused(method, params).await,
            ErrorCode::Forbidden,
            "{method}"
        );
    }

    let created = guest
        .forwarded(
            "agent.create.request",
            json!({"config": {"provider": "codex", "cwd": guest.outside()},
                "workspaceId": sibling}),
        )
        .await;
    assert_eq!(created["workspaceId"], json!(sibling));
    assert_eq!(created["config"]["cwd"], json!(guest.path("sibling")));
    assert!(created["labels"][GUEST_SHARE].is_string());
    for params in [
        json!({"config": {"provider": "codex"}}),
        json!({"config": {"provider": "codex"}, "workspaceId": guest.fixture.other}),
        json!({"config": {"provider": "codex"}, "workspaceId": sibling,
            "worktree": {"mode": "new"}}),
    ] {
        assert_eq!(
            guest.refused("agent.create.request", params).await,
            ErrorCode::Forbidden
        );
    }
}

#[tokio::test]
async fn project_agent_directories_cover_the_project_workspaces() {
    let guest = Guest::with(ResourceKind::Project).await;
    let all = guest.forwarded("agent.list.request", json!({})).await;
    let mut ids: Vec<&str> = all["filter"]["workspaceIds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap())
        .collect();
    ids.sort_unstable();
    let mut expected = vec![
        guest.fixture.shared.as_str(),
        guest.fixture.sibling.as_str(),
    ];
    expected.sort_unstable();
    assert_eq!(ids, expected);
    let narrowed = guest
        .forwarded(
            "agent.history.get.request",
            json!({"filter": {"workspaceIds": [guest.fixture.sibling, guest.fixture.other]}}),
        )
        .await;
    assert_eq!(
        narrowed["filter"]["workspaceIds"],
        json!([guest.fixture.sibling])
    );
    let outside = guest
        .forwarded(
            "agent.list.request",
            json!({"filter": {"workspaceIds": [guest.fixture.other]}}),
        )
        .await;
    assert_eq!(outside["filter"]["workspaceIds"], json!([]));
}

#[tokio::test]
async fn node_shares_reach_every_project_but_not_host_management() {
    let guest = Guest::with(ResourceKind::Node).await;
    for (method, params) in [
        ("project.add.request", json!({"cwd": guest.outside()})),
        (
            "project.rename.request",
            json!({"projectId": guest.fixture.other_project}),
        ),
        ("project.list.request", json!({"sync": {}})),
        ("workspace.list.request", json!({"subscribe": {}})),
        (
            "workspace.get.request",
            json!({"workspaceId": guest.fixture.other}),
        ),
        ("fs.explorer.request", json!({"cwd": guest.outside()})),
        ("fs.explorer.request", json!({"cwd": "relative"})),
        (
            "workspace.worktree.create.request",
            json!({"cwd": guest.outside()}),
        ),
        (
            "workspace.create.request",
            json!({"source": {"kind": "directory", "path": guest.outside()}}),
        ),
        ("workspace.open.request", json!({"cwd": guest.outside()})),
        ("terminal.list.request", json!({})),
        ("agent.list.request", json!({"filter": {"labels": {}}})),
    ] {
        let forwarded = guest.forwarded(method, params.clone()).await;
        assert_eq!(forwarded, params, "{method}");
    }
    for method in [
        "daemon.get_status.request",
        "daemon.config.get.request",
        "server.restart.request",
        "diagnostics.request",
        "schedule.create.request",
        "bot.list.request",
        "browser.host.register.request",
        "relay.status.request",
        "publication.authorize.request",
        "push.unregister.request",
        "voice.abort.request",
        "share.list.request",
        "share.set.request",
        "agent.configure",
        "agent.skills.get_status.request",
        "agent.refresh.request",
        "provider.usage.list.request",
        "provider.sessions.recent.list.request",
        "session.events.set_subscription.request",
    ] {
        assert_eq!(
            guest.refused(method, json!({})).await,
            ErrorCode::Forbidden,
            "{method}"
        );
    }
    assert_eq!(
        guest
            .refused(
                "workspace.create.request",
                json!({"source": {"kind": "directory", "path": guest.root()},
                    "agent": {"provider": "codex"}}),
            )
            .await,
        ErrorCode::Forbidden
    );
}

#[tokio::test]
async fn node_shares_keep_the_agent_rules() {
    let guest = Guest::with(ResourceKind::Node).await;
    let created = guest
        .forwarded(
            "agent.create.request",
            json!({"config": {"provider": "codex", "cwd": guest.outside()},
                "workspaceId": guest.fixture.other, "worktree": {"mode": "new"}}),
        )
        .await;
    assert_eq!(created["workspaceId"], json!(guest.fixture.other));
    assert_eq!(created["config"]["cwd"], json!(guest.outside()));
    assert!(created["labels"][GUEST_SHARE].is_string());
    assert!(created["labels"][GUEST_USER].is_string());
    for params in [
        json!({"config": {"provider": "codex"}, "env": {"A": "1"}}),
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
    // Driving still requires an Agent this share's guests created.
    for (method, params) in [
        (
            "agent.message.send.request",
            json!({"agentId": "owner-agent", "text": "hi"}),
        ),
        ("agent.cancel.request", json!({"agentId": "owner-agent"})),
        (
            "agent.update.request",
            json!({"agentId": "owner-agent", "labels": {}}),
        ),
    ] {
        assert_eq!(guest.refused(method, params).await, ErrorCode::Forbidden);
    }
    let decision = guest
        .decide("terminal.create.request", json!({"cwd": guest.outside()}))
        .await;
    let Decision::Private(request, Private::TerminalCreate) = decision else {
        panic!("terminal creation was not private: {decision:?}");
    };
    assert_eq!(request.params, json!({"cwd": guest.outside()}));
}
