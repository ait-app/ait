//! In-memory registry of Bot and subtree Ait MCP credentials.
//!
//! Credentials exist only in daemon memory, indexed by SHA-256 digest, and are regenerated on
//! every daemon start. Authorization is read from the latest published catalog on every
//! request, so permission changes apply without reissuing credentials.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::sync::{Arc, PoisonError, RwLock};

use domain::bot::permissions::{BotPermissions, Holder};
use domain::bot::{BOT_SCOPE_LABEL, BOT_TICKET_LABEL, BotState, Catalog, SessionOwner};
use domain::{BotId, ConversationId, TicketId};
use provider::ports::mcp_scope::{McpAccess, McpScopes, McpSubject};
use secrecy::SecretString;
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Most credentials held at once; issuing beyond it evicts the oldest.
const CAPACITY: usize = 4096;
/// Hex length of an issued credential (two simple version 4 UUIDs).
const CREDENTIAL_LENGTH: usize = 64;

type Fingerprint = [u8; 32];

/// Who presented a scoped credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallerKind {
    /// A Bot's own hidden conversation session.
    Bot {
        /// Bot generation the credential was issued to.
        bot_id: BotId,
        /// Conversation the hidden session belongs to; work it starts reports back here.
        conversation_id: ConversationId,
        /// Hidden native session identity.
        session_id: String,
    },
    /// An Agent started inside a Bot's scope, or one of its descendants.
    Subtree {
        /// Host Agent identity.
        agent_id: String,
        /// The Bot ticket the Agent works on, from its daemon-written `ait.bot.ticket` label
        /// when its session started; `None` without a valid label.
        ticket: Option<TicketId>,
    },
}

impl CallerKind {
    fn agent_id(&self) -> &str {
        match self {
            Self::Bot { session_id, .. } => session_id,
            Self::Subtree { agent_id, .. } => agent_id,
        }
    }

    /// The tool access this credential has: a Bot session, a subtree Agent, or a subtree Agent
    /// working on a ticket.
    #[must_use]
    pub const fn holder(&self) -> Holder {
        match self {
            Self::Bot { .. } => Holder::Bot,
            Self::Subtree { ticket: None, .. } => Holder::Subtree,
            Self::Subtree {
                ticket: Some(_), ..
            } => Holder::Worker,
        }
    }
}

/// Resolved identity of a scoped credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    /// Root Workspace whose Bot defines the caller's authorization.
    pub root: String,
    /// Bot session or subtree Agent.
    pub kind: CallerKind,
}

/// Current authorization for a root Workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    /// Tool groups and mode cap.
    pub permissions: BotPermissions,
    /// The root Workspace and the unarchived worktrees its Bot created.
    pub workspaces: BTreeSet<String>,
}

/// Published facts about a root Workspace's non-retired Bot.
struct Root {
    bot_id: BotId,
    active: bool,
    permissions: BotPermissions,
    workspaces: BTreeSet<String>,
}

struct Issued {
    caller: Caller,
    sequence: u64,
}

#[derive(Default)]
struct Inner {
    roots: HashMap<String, Root>,
    credentials: HashMap<Fingerprint, Issued>,
    agents: HashMap<String, Fingerprint>,
    order: BTreeMap<u64, Fingerprint>,
    next_sequence: u64,
}

impl Inner {
    /// Whether `bot_id` is still `root`'s current Bot and is active.
    fn admits(&self, root: &str, bot_id: BotId) -> bool {
        self.roots
            .get(root)
            .is_some_and(|current| current.bot_id == bot_id && current.active)
    }

    fn revoke(&mut self, agent_id: &str) {
        if let Some(fingerprint) = self.agents.remove(agent_id)
            && let Some(issued) = self.credentials.remove(&fingerprint)
        {
            self.order.remove(&issued.sequence);
        }
    }

    /// Issue a credential for `caller`, whose Agent holds none; evicts the oldest at capacity.
    fn issue(&mut self, caller: Caller) -> SecretString {
        if self.credentials.len() >= CAPACITY
            && let Some((_, oldest)) = self.order.pop_first()
            && let Some(evicted) = self.credentials.remove(&oldest)
        {
            self.agents.remove(evicted.caller.kind.agent_id());
        }
        let secret = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let fingerprint = fingerprint(&secret);
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        self.order.insert(sequence, fingerprint);
        self.agents
            .insert(caller.kind.agent_id().to_owned(), fingerprint);
        self.credentials
            .insert(fingerprint, Issued { caller, sequence });
        SecretString::from(secret)
    }
}

fn fingerprint(secret: &str) -> Fingerprint {
    Sha256::digest(secret.as_bytes()).into()
}

/// Daemon-wide credential registry shared by the Bot worker, Provider and the MCP route.
pub struct Scopes {
    inner: RwLock<Inner>,
}

impl fmt::Debug for Scopes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let credentials = self.read().credentials.len();
        formatter
            .debug_struct("Scopes")
            .field("credentials", &credentials)
            .finish_non_exhaustive()
    }
}

impl Scopes {
    /// Create an empty registry; nothing is valid until a catalog is published.
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: RwLock::new(Inner::default()),
        })
    }

    /// Resolve a presented bearer secret; `None` if unknown, superseded or stale.
    ///
    /// A Bot credential additionally requires its Bot to still be the root Workspace's
    /// current Bot and active.
    #[must_use]
    pub fn resolve(&self, secret: &str) -> Option<Caller> {
        if secret.len() != CREDENTIAL_LENGTH {
            return None;
        }
        let inner = self.read();
        let caller = &inner.credentials.get(&fingerprint(secret))?.caller;
        match &caller.kind {
            CallerKind::Bot { bot_id, .. } if !inner.admits(&caller.root, *bot_id) => None,
            CallerKind::Bot { .. } | CallerKind::Subtree { .. } => Some(caller.clone()),
        }
    }

    /// Current grant for root Workspace `root`; observe-only when it has no non-retired Bot.
    ///
    /// The returned Workspaces always include `root` itself.
    #[must_use]
    pub fn grant(&self, root: &str) -> Grant {
        let inner = self.read();
        let (permissions, mut workspaces) = inner.roots.get(root).map_or_else(
            || (BotPermissions::observe_only(), BTreeSet::new()),
            |current| (current.permissions.clone(), current.workspaces.clone()),
        );
        workspaces.insert(root.to_owned());
        Grant {
            permissions,
            workspaces,
        }
    }

    /// Whether `workspace` had a current, active Bot in the last published catalog; a cheap
    /// filter before the Bot worker decides authoritatively.
    pub(crate) fn has_active_bot(&self, workspace: &str) -> bool {
        self.read()
            .roots
            .get(workspace)
            .is_some_and(|root| root.active)
    }

    /// Replace the authorization snapshot and revoke Bot credentials that lost their Bot.
    pub(crate) fn publish(&self, catalog: &Catalog) {
        let roots = catalog
            .bots
            .iter()
            .filter(|bot| !matches!(bot.state, BotState::Retired { .. }))
            .map(|bot| {
                let root = Root {
                    bot_id: bot.id,
                    active: matches!(bot.state, BotState::Active),
                    permissions: bot.permissions.clone(),
                    workspaces: bot.workspaces.clone(),
                };
                (bot.workspace_id.clone(), root)
            })
            .collect();
        let mut inner = self.write();
        inner.roots = roots;
        let stale: Vec<String> = inner
            .credentials
            .values()
            .filter_map(|issued| match &issued.caller.kind {
                CallerKind::Bot {
                    bot_id, session_id, ..
                } if !inner.admits(&issued.caller.root, *bot_id) => Some(session_id.clone()),
                CallerKind::Bot { .. } | CallerKind::Subtree { .. } => None,
            })
            .collect();
        for agent_id in stale {
            inner.revoke(&agent_id);
        }
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Inner> {
        self.inner.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, Inner> {
        self.inner.write().unwrap_or_else(PoisonError::into_inner)
    }
}

impl McpScopes for Scopes {
    /// Issue a Bot credential for an active Bot's hidden session, a subtree credential for
    /// an Agent carrying the Bot scope label (recording its ticket label, if any, and marking
    /// the credential as a worker's), and leave
    /// every other session on the shared token. Any previous credential of the same Agent is revoked first.
    fn access(&self, subject: &McpSubject<'_>) -> McpAccess {
        let owner = subject
            .owner
            .filter(|_| subject.internal)
            .and_then(|owner| SessionOwner::deserialize(owner).ok());
        let mut inner = self.write();
        inner.revoke(subject.agent_id);
        if let Some(owner) = owner {
            if !inner.admits(&owner.workspace_id, owner.bot_id) {
                return McpAccess::Denied;
            }
            let caller = Caller {
                root: owner.workspace_id,
                kind: CallerKind::Bot {
                    bot_id: owner.bot_id,
                    conversation_id: owner.conversation_id,
                    session_id: subject.agent_id.to_owned(),
                },
            };
            return McpAccess::Scoped {
                token: inner.issue(caller),
                bot_session: true,
                worker: false,
            };
        }
        let Some(root) = subject.labels.get(BOT_SCOPE_LABEL) else {
            return McpAccess::Shared;
        };
        let ticket = subject
            .labels
            .get(BOT_TICKET_LABEL)
            .and_then(|ticket| ticket.parse().ok());
        let caller = Caller {
            root: root.clone(),
            kind: CallerKind::Subtree {
                agent_id: subject.agent_id.to_owned(),
                ticket,
            },
        };
        McpAccess::Scoped {
            token: inner.issue(caller),
            bot_session: false,
            worker: ticket.is_some(),
        }
    }

    fn release(&self, agent_id: &str) {
        self.write().revoke(agent_id);
    }
}

#[cfg(test)]
mod tests;
