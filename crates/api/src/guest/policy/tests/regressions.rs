use super::*;

mod creation;

#[tokio::test]
async fn node_guests_can_read_but_cannot_start_persistent_automation() {
    let guest = Guest::with(ResourceKind::Node).await;
    for method in [
        "schedule.create.request",
        "schedule.update.request",
        "schedule.resume.request",
        "schedule.run_once.request",
        "bot.setup.request",
        "bot.update.request",
        "bot.permissions.update.request",
        "bot.channel.create.request",
        "bot.channel.update.request",
        "bot.channel.feishu.scan.start.request",
        "bot.channel.feishu.scan.status.request",
        "bot.channel.feishu.scan.cancel.request",
        "bot.channel.feishu.connect.request",
        "bot.channel.telegram.pair.start.request",
        "bot.channel.telegram.pair.status.request",
        "bot.channel.telegram.pair.cancel.request",
        "bot.channel.weixin.scan.start.request",
        "bot.channel.weixin.scan.status.request",
        "bot.channel.weixin.scan.cancel.request",
        "bot.channel.qq.scan.start.request",
        "bot.channel.qq.scan.status.request",
        "bot.channel.qq.scan.cancel.request",
        "bot.channel.qq.connect.request",
        "bot.channel.qq.pair.request",
        "bot.channel.imessage.pair.start.request",
        "bot.channel.imessage.pair.status.request",
        "bot.channel.imessage.pair.cancel.request",
        "bot.channel.webhook.create.request",
        "bot.channel.webhook.rotate.request",
        "bot.message.send.request",
        "bot.conversation.open.request",
        "bot.pause.request",
        "bot.resume.request",
        "bot.remove.request",
        "bot.memory.update.request",
    ] {
        assert_eq!(
            guest.refused(method, json!({})).await,
            ErrorCode::Forbidden,
            "{method}"
        );
    }
    for method in [
        "schedule.list.request",
        "schedule.inspect.request",
        "bot.get.request",
    ] {
        guest.forwarded(method, json!({})).await;
    }
}

#[tokio::test]
async fn guest_bot_access_is_read_only_v2() {
    let guest = Guest::with(ResourceKind::Node).await;
    for method in [
        "bot.get.request",
        "bot.channel.list.request",
        "bot.channel.types.request",
        "bot.channel.status.request",
        "bot.conversation.list.request",
        "bot.history.get.request",
        "bot.turn.get.request",
        "bot.memory.get.request",
        "bot.notifications.list.request",
    ] {
        guest.forwarded(method, json!({})).await;
    }
    for method in [
        "bot.list.request",
        "bot.channel.delete.request",
        "bot.conversation.reset.request",
    ] {
        assert_eq!(
            guest.refused(method, json!({})).await,
            ErrorCode::Forbidden,
            "{method}"
        );
    }
}

#[tokio::test]
async fn project_guests_cannot_archive_an_independently_shared_workspace() {
    let guest = Guest::with(ResourceKind::Project).await;
    guest.fixture.accept_share().await;
    for (method, params) in [
        (
            "workspace.archive.request",
            json!({"workspaceId": guest.fixture.shared}),
        ),
        (
            "workspace.worktree.archive.request",
            json!({"workspaceId": guest.fixture.shared}),
        ),
        (
            "workspace.worktree.archive.request",
            json!({"worktreePath": guest.root()}),
        ),
        (
            "workspace.worktree.archive.request",
            json!({"worktreePath": guest.root(), "scope": "workspace"}),
        ),
        (
            "workspace.worktree.archive.request",
            json!({"workspaceId": guest.fixture.sibling, "worktreePath": guest.root(), "scope": "worktree"}),
        ),
    ] {
        assert_eq!(guest.refused(method, params).await, ErrorCode::Forbidden);
    }
    guest
        .forwarded(
            "workspace.archive.request",
            json!({"workspaceId": guest.fixture.sibling}),
        )
        .await;
    assert!(
        guest
            .fixture
            .api
            .shared
            .shares
            .has_settings(ResourceKind::Workspace, &guest.fixture.shared)
    );
}

#[tokio::test]
async fn scoped_file_mutations_cannot_replace_project_configuration() {
    for kind in [ResourceKind::Workspace, ResourceKind::Project] {
        let guest = Guest::with(kind).await;
        std::fs::write(guest.fixture.shared_path().join("file.txt"), "content").unwrap();
        // The config store falls back to `paseo.json` when `ait.json` is missing.
        for config in ["ait.json", "paseo.json"] {
            for (method, params) in [
                (
                    "fs.file.write.request",
                    json!({"cwd": guest.root(), "path": config}),
                ),
                (
                    "fs.entry.create.request",
                    json!({"cwd": guest.root(), "parentPath": ".", "name": config}),
                ),
                (
                    "fs.entry.rename.request",
                    json!({"cwd": guest.root(), "path": "file.txt", "name": config}),
                ),
                (
                    "fs.entry.delete.request",
                    json!({"cwd": guest.root(), "path": config}),
                ),
            ] {
                assert_eq!(
                    guest.refused(method, params).await,
                    ErrorCode::Forbidden,
                    "{method} {config}"
                );
            }
        }
        guest
            .forwarded(
                "fs.file.write.request",
                json!({"cwd": guest.root(), "path": "file.txt"}),
            )
            .await;
    }
}

#[cfg(unix)]
#[tokio::test]
async fn trailing_whitespace_and_symlink_aliases_do_not_bypass_path_checks() {
    let guest = Guest::new().await;
    let root = guest.fixture.shared_path();
    std::fs::create_dir(root.join("link ")).unwrap();
    std::os::unix::fs::symlink(guest.outside(), root.join("link")).unwrap();
    assert_eq!(
        guest
            .refused("fs.explorer.request", json!({"cwd": root.join("link ")}))
            .await,
        ErrorCode::Forbidden
    );
    std::fs::write(root.join("ait.json"), "{}").unwrap();
    std::os::unix::fs::symlink(root.join("ait.json"), root.join("alias")).unwrap();
    assert_eq!(
        guest
            .refused(
                "fs.file.write.request",
                json!({"cwd": guest.root(), "path": "alias"})
            )
            .await,
        ErrorCode::Forbidden
    );
}

#[tokio::test]
async fn explicit_forge_identity_requires_a_matching_checkout() {
    let guest = Guest::new().await;
    for method in [
        "checkout.pr.timeline.request",
        "checkout.forge.get_check_details.request",
        "checkout.github.get_check_details.request",
    ] {
        // An unavailable resolver must not forward an unchecked repository identity.
        assert!(matches!(
            guest
                .decide(
                    method,
                    json!({"cwd": guest.root(), "repoOwner": "private", "repoName": "other"})
                )
                .await,
            Decision::Refuse(_, _)
        ));
        assert_eq!(
            guest
                .refused(method, json!({"cwd": guest.root(), "repoOwner": "private"}))
                .await,
            ErrorCode::Forbidden
        );
    }
}
