use std::collections::{BTreeMap, BTreeSet};

use domain::bot::permissions::{BotPermissions, ToolGroup};
use domain::bot::{
    BOT_SCOPE_LABEL, Bot, BotState, Catalog, ExecutionConfig, PauseReason, SessionOwner,
};
use domain::{BotId, ChannelId, ConversationId};
use model::bot::Store;
use provider::ports::mcp_scope::{McpAccess, McpScopes, McpSubject};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};

use super::*;
use crate::test_support::{Fixture, WORKSPACE, setup};

fn active_bot(workspace: &str) -> Bot {
    Bot {
        id: BotId::generate(),
        workspace_id: workspace.to_owned(),
        execution: ExecutionConfig {
            provider: "test".to_owned(),
            model: None,
            mode_id: None,
            thinking: None,
        },
        builtin_channel_id: ChannelId::generate(),
        state: BotState::Active,
        created_at: "2026-11-06T00:00:00Z".to_owned(),
        permissions: BotPermissions::default(),
        workspaces: BTreeSet::new(),
        wake_scope: domain::bot::WakeScope::Own,
    }
}

fn catalog_with(bot: Bot) -> Catalog {
    Catalog {
        bots: vec![bot],
        channels: Vec::new(),
    }
}

fn subject<'a>(
    agent_id: &'a str,
    internal: bool,
    owner: Option<&'a Value>,
    labels: &'a BTreeMap<String, String>,
) -> McpSubject<'a> {
    McpSubject {
        agent_id,
        internal,
        owner,
        labels,
    }
}

fn scoped_token(access: McpAccess) -> SecretString {
    match access {
        McpAccess::Scoped { token, .. } => token,
        other => panic!("expected a scoped credential, got {other:?}"),
    }
}

fn bot_token(access: McpAccess) -> SecretString {
    match access {
        McpAccess::Scoped {
            token,
            bot_session: true,
        } => token,
        other => panic!("expected a Bot session credential, got {other:?}"),
    }
}

fn owner_of(bot: &Bot) -> Value {
    serde_json::to_value(SessionOwner {
        bot_id: bot.id,
        conversation_id: ConversationId::generate(),
        workspace_id: bot.workspace_id.clone(),
    })
    .unwrap()
}

fn scope_labels(root: &str) -> BTreeMap<String, String> {
    BTreeMap::from([(BOT_SCOPE_LABEL.to_owned(), root.to_owned())])
}

#[test]
fn issued_credentials_resolve_to_their_caller_and_rotate_on_reissue() {
    let scopes = Scopes::new();
    scopes.publish(&catalog_with(active_bot(WORKSPACE)));
    let labels = scope_labels(WORKSPACE);
    let first = scoped_token(scopes.access(&subject("child", false, None, &labels)));
    let caller = scopes.resolve(first.expose_secret()).unwrap();
    assert_eq!(caller.root, WORKSPACE);
    assert!(matches!(caller.kind, CallerKind::Subtree { ref agent_id } if agent_id == "child"));
    let second = scoped_token(scopes.access(&subject("child", false, None, &labels)));
    assert_ne!(first.expose_secret(), second.expose_secret());
    assert!(scopes.resolve(first.expose_secret()).is_none());
    assert_eq!(second.expose_secret().len(), 64);
    assert!(
        second
            .expose_secret()
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    );
    assert!(scopes.resolve(&"0".repeat(64)).is_none());
    assert!(scopes.resolve("").is_none());
}

#[test]
fn bot_credential_is_rejected_after_pause_remove_or_new_generation() {
    let scopes = Scopes::new();
    let mut bot = active_bot(WORKSPACE);
    scopes.publish(&catalog_with(bot.clone()));
    let owner = owner_of(&bot);
    let no_labels = BTreeMap::new();
    let session = subject("session", true, Some(&owner), &no_labels);

    let token = bot_token(scopes.access(&session));
    let caller = scopes.resolve(token.expose_secret()).unwrap();
    assert_eq!(caller.root, WORKSPACE);
    let conversation = owner["conversationId"].as_str().unwrap();
    assert!(matches!(
        caller.kind,
        CallerKind::Bot { bot_id, conversation_id, ref session_id }
            if bot_id == bot.id
                && conversation_id.to_string() == conversation
                && session_id == "session"
    ));

    bot.state = BotState::Paused {
        reason: PauseReason::User,
    };
    scopes.publish(&catalog_with(bot.clone()));
    assert!(scopes.resolve(token.expose_secret()).is_none());
    assert!(matches!(scopes.access(&session), McpAccess::Denied));

    bot.state = BotState::Active;
    scopes.publish(&catalog_with(bot.clone()));
    assert!(scopes.resolve(token.expose_secret()).is_none());
    let resumed = bot_token(scopes.access(&session));
    assert!(scopes.resolve(resumed.expose_secret()).is_some());

    let mut retired = bot.clone();
    retired.state = BotState::Retired {
        at: "2026-11-06T01:00:00Z".to_owned(),
    };
    let replacement = active_bot(WORKSPACE);
    scopes.publish(&Catalog {
        bots: vec![retired, replacement.clone()],
        channels: Vec::new(),
    });
    assert!(scopes.resolve(resumed.expose_secret()).is_none());
    assert!(matches!(scopes.access(&session), McpAccess::Denied));
    let next_owner = owner_of(&replacement);
    let next = bot_token(scopes.access(&subject("next", true, Some(&next_owner), &no_labels)));
    assert!(scopes.resolve(next.expose_secret()).is_some());
}

#[test]
fn subtree_credential_follows_current_bot_permissions() {
    let scopes = Scopes::new();
    let mut bot = active_bot(WORKSPACE);
    bot.permissions = BotPermissions {
        groups: BTreeSet::from([ToolGroup::Observe, ToolGroup::Agents]),
        modes: BTreeMap::from([("codex".to_owned(), vec!["auto".to_owned()])]),
    };
    scopes.publish(&catalog_with(bot.clone()));
    let labels = scope_labels(WORKSPACE);
    let token = scoped_token(scopes.access(&subject("child", false, None, &labels)));
    let caller = scopes.resolve(token.expose_secret()).unwrap();
    assert_eq!(scopes.grant(&caller.root).permissions, bot.permissions);

    bot.permissions = BotPermissions {
        groups: BTreeSet::from([ToolGroup::Observe, ToolGroup::Terminals]),
        modes: BTreeMap::new(),
    };
    scopes.publish(&catalog_with(bot.clone()));
    let caller = scopes.resolve(token.expose_secret()).unwrap();
    assert_eq!(scopes.grant(&caller.root).permissions, bot.permissions);
}

#[test]
fn subtree_without_bot_falls_back_to_observe_only() {
    let scopes = Scopes::new();
    let labels = scope_labels(WORKSPACE);
    let token = scoped_token(scopes.access(&subject("child", false, None, &labels)));
    let observe_only = Grant {
        permissions: BotPermissions::observe_only(),
        workspaces: BTreeSet::from([WORKSPACE.to_owned()]),
    };
    assert!(scopes.resolve(token.expose_secret()).is_some());
    assert_eq!(scopes.grant(WORKSPACE), observe_only);

    let mut bot = active_bot(WORKSPACE);
    bot.workspaces = BTreeSet::from(["wks_worktree".to_owned()]);
    scopes.publish(&catalog_with(bot.clone()));
    assert_eq!(
        scopes.grant(WORKSPACE).workspaces,
        BTreeSet::from([WORKSPACE.to_owned(), "wks_worktree".to_owned()])
    );

    bot.state = BotState::Retired {
        at: "2026-11-06T01:00:00Z".to_owned(),
    };
    scopes.publish(&catalog_with(bot));
    assert!(scopes.resolve(token.expose_secret()).is_some());
    assert_eq!(scopes.grant(WORKSPACE), observe_only);
}

#[test]
fn unrelated_internal_and_unlabelled_subjects_use_shared_access() {
    let scopes = Scopes::new();
    let bot = active_bot(WORKSPACE);
    scopes.publish(&catalog_with(bot.clone()));
    let owner = owner_of(&bot);
    let foreign = json!({"kind":"voice","sessionId":"voice-1"});
    let no_labels = BTreeMap::new();
    let other_labels = BTreeMap::from([("team".to_owned(), "ops".to_owned())]);
    for subject in [
        subject("internal", true, None, &no_labels),
        subject("voice", true, Some(&foreign), &no_labels),
        subject("public", false, None, &no_labels),
        subject("labelled", false, None, &other_labels),
        subject("public-owner", false, Some(&owner), &no_labels),
    ] {
        assert!(
            matches!(scopes.access(&subject), McpAccess::Shared),
            "{}",
            subject.agent_id
        );
    }
}

#[test]
fn released_credentials_stop_resolving() {
    let scopes = Scopes::new();
    let bot = active_bot(WORKSPACE);
    scopes.publish(&catalog_with(bot.clone()));
    let owner = owner_of(&bot);
    let labels = scope_labels(WORKSPACE);
    let no_labels = BTreeMap::new();
    let child = scoped_token(scopes.access(&subject("child", false, None, &labels)));
    let session = bot_token(scopes.access(&subject("session", true, Some(&owner), &no_labels)));

    scopes.release("child");
    scopes.release("session");

    assert!(scopes.resolve(child.expose_secret()).is_none());
    assert!(scopes.resolve(session.expose_secret()).is_none());
    scopes.release("child");
    scopes.release("unknown");
}

#[test]
fn archived_worktrees_leave_the_grant() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    drop(state);
    let mut catalog = fixture.store.catalog().unwrap();
    catalog.bots[0].workspaces =
        BTreeSet::from(["wks_worktree_a".to_owned(), "wks_worktree_b".to_owned()]);
    fixture.store.save_catalog(&catalog).unwrap();
    let mut state = fixture.state();
    assert_eq!(fixture.scopes.grant(WORKSPACE).workspaces.len(), 3);

    state.retire(&["wks_worktree_a".to_owned()]).unwrap();

    let expected = BTreeSet::from([WORKSPACE.to_owned(), "wks_worktree_b".to_owned()]);
    assert_eq!(fixture.scopes.grant(WORKSPACE).workspaces, expected);
    let persisted = fixture.store.catalog().unwrap();
    assert_eq!(
        persisted.bots[0].workspaces,
        BTreeSet::from(["wks_worktree_b".to_owned()])
    );
    assert_eq!(persisted.bots[0].state, BotState::Active);
}

#[test]
fn credentials_are_bounded_and_absent_from_debug_output() {
    let scopes = Scopes::new();
    let labels = scope_labels(WORKSPACE);
    let agents: Vec<String> = (0..=CAPACITY)
        .map(|index| format!("agent-{index}"))
        .collect();
    let tokens: Vec<SecretString> = agents
        .iter()
        .map(|agent| scoped_token(scopes.access(&subject(agent, false, None, &labels))))
        .collect();

    assert!(scopes.resolve(tokens[0].expose_secret()).is_none());
    assert!(scopes.resolve(tokens[1].expose_secret()).is_some());
    assert!(scopes.resolve(tokens[CAPACITY].expose_secret()).is_some());
    let debug = format!("{scopes:?}");
    assert!(debug.contains(&CAPACITY.to_string()));
    assert!(
        tokens
            .iter()
            .all(|token| !debug.contains(token.expose_secret()))
    );
}
