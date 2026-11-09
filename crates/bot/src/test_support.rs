use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use domain::bot::SessionOwner;
use model::ErrorCode;
use model::workspace::registry::{ProjectRegistry, WorkspaceRegistry};
use persistence::storage::bot::FileStore;
use persistence::storage::registry::{FileBackedProjectRegistry, FileBackedWorkspaceRegistry};
use provider::service::bot_execution::Action;
use secrecy::SecretString;
use serde_json::{Value, json};

use crate::mcp::Scopes;
use crate::ports::{Credentials, Executor};
use crate::service::{Bots, Dependencies};
use crate::state::State;

#[derive(Debug, Default)]
pub(crate) struct Native {
    pub(crate) calls: Mutex<Vec<(SessionOwner, Action)>>,
    messages: Mutex<BTreeMap<String, String>>,
    pub(crate) hold: AtomicBool,
    pub(crate) approval: AtomicBool,
    pub(crate) fail_denial: AtomicBool,
    pub(crate) status: Mutex<Option<Value>>,
    /// Scripted `Action::Create` results, in order; a new system-prompt session once exhausted.
    pub(crate) creates: Mutex<VecDeque<Value>>,
}
#[async_trait::async_trait]
impl Executor for Native {
    async fn execute(&self, owner: SessionOwner, action: Action) -> Result<Value, ErrorCode> {
        self.calls.lock().unwrap().push((owner, action.clone()));
        Ok(match action {
            Action::Create { .. } => {
                self.creates.lock().unwrap().pop_front().unwrap_or_else(
                    || json!({"created":true,"instructionDelivery":"system_prompt"}),
                )
            }
            Action::Close { .. } | Action::Cancel { .. } => json!({"ok":true}),
            Action::Release { .. } => json!({"released":true}),
            Action::Send { agent_id, text, .. } => {
                self.messages.lock().unwrap().insert(agent_id, text);
                json!({"accepted":true})
            }
            Action::Permission { .. } => {
                if self.fail_denial.load(Ordering::SeqCst) {
                    return Err(ErrorCode::AgentIo);
                }
                self.approval.store(false, Ordering::SeqCst);
                json!({"resolved":true})
            }
            Action::Status { agent_id } => {
                if let Some(status) = self.status.lock().unwrap().as_ref() {
                    return Ok(status.clone());
                }
                if self.approval.load(Ordering::SeqCst) {
                    json!({"status":"permission","final":{"pendingPermissions":[{"id":"approval-1","options":["allow","deny"]}]}})
                } else if self.hold.load(Ordering::SeqCst) {
                    json!({"status":"running"})
                } else {
                    json!({"status":"idle","lastMessage":format!("reply:{}",self.messages.lock().unwrap().get(&agent_id).unwrap())})
                }
            }
        })
    }
}
#[derive(Debug)]
struct NoCredentials;
impl Credentials for NoCredentials {
    fn resolve(&self, _: &str) -> Result<SecretString, ErrorCode> {
        Err(ErrorCode::BotIo)
    }
}

/// Credentials port that keeps stored secrets in memory.
#[derive(Debug, Default)]
pub(crate) struct MemorySecrets(Mutex<BTreeMap<domain::ChannelId, String>>);
impl MemorySecrets {
    pub(crate) fn stored(&self) -> Vec<String> {
        self.0.lock().unwrap().values().cloned().collect()
    }
}
impl Credentials for MemorySecrets {
    fn resolve(&self, _: &str) -> Result<SecretString, ErrorCode> {
        Err(ErrorCode::BotIo)
    }
    fn store(&self, channel: domain::ChannelId, secret: &SecretString) -> Result<(), ErrorCode> {
        use secrecy::ExposeSecret;
        self.0
            .lock()
            .unwrap()
            .insert(channel, secret.expose_secret().to_owned());
        Ok(())
    }
    fn delete(&self, channel: domain::ChannelId) -> Result<(), ErrorCode> {
        self.0.lock().unwrap().remove(&channel);
        Ok(())
    }
}

pub(crate) struct Fixture {
    pub(crate) root: tempfile::TempDir,
    pub(crate) store: Arc<FileStore>,
    pub(crate) workspaces: Arc<FileBackedWorkspaceRegistry>,
    pub(crate) projects: Arc<FileBackedProjectRegistry>,
    pub(crate) native: Arc<Native>,
    pub(crate) scopes: Arc<Scopes>,
}
impl Fixture {
    pub(crate) fn new() -> Self {
        Self::build(false)
    }
    /// Fixture whose [`WORKSPACE`] directory is `root/ws`, so tests can delete it.
    pub(crate) fn with_workspace_subdir() -> Self {
        Self::build(true)
    }
    fn build(workspace_subdir: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().to_str().unwrap();
        let workspace_dir = if workspace_subdir {
            let dir = root.path().join("ws");
            std::fs::create_dir(&dir).unwrap();
            dir
        } else {
            root.path().to_path_buf()
        };
        let projects = Arc::new(FileBackedProjectRegistry::new(
            root.path().join("projects.json"),
        ));
        projects.upsert(&serde_json::from_value(json!({"projectId":"project-1","rootPath":cwd,"kind":"non_git","displayName":"project", "createdAt":"2026-10-12T00:00:00Z","updatedAt":"2026-10-12T00:00:00Z","archivedAt":null})).unwrap()).unwrap();
        let workspaces = Arc::new(FileBackedWorkspaceRegistry::new(
            root.path().join("workspaces.json"),
        ));
        let other = root.path().join("other");
        std::fs::create_dir(&other).unwrap();
        for (id, cwd) in [
            (WORKSPACE, workspace_dir.to_str().unwrap()),
            (OTHER_WORKSPACE, other.to_str().unwrap()),
        ] {
            workspaces.upsert(&serde_json::from_value(json!({"workspaceId":id,"projectId":"project-1","cwd":cwd,"kind":"local_checkout","displayName":"workspace", "createdAt":"2026-10-12T00:00:00Z","updatedAt":"2026-10-12T00:00:00Z","archivedAt":null})).unwrap(), domain::workspace::registry::WorkspaceMutationContext::default()).unwrap();
        }
        Self {
            store: Arc::new(FileStore::new(root.path().join("bots"))),
            root,
            workspaces,
            projects,
            native: Arc::default(),
            scopes: Scopes::new(),
        }
    }
    pub(crate) fn state(&self) -> State {
        State::load(
            self.store.clone(),
            self.workspaces.clone(),
            self.projects.clone(),
            self.scopes.clone(),
        )
        .unwrap()
    }
    /// The persisted domain channel behind a client channel view, which omits the owner.
    pub(crate) fn channel(&self, view: &Value) -> domain::bot::Channel {
        let id = view["id"].as_str().unwrap();
        self.state()
            .channels()
            .into_iter()
            .find(|channel| channel.id.to_string() == id)
            .unwrap()
    }
    pub(crate) fn bots(&self) -> Bots {
        self.bots_with(Arc::new(NoCredentials))
    }
    pub(crate) fn bots_with(&self, credentials: Arc<dyn Credentials>) -> Bots {
        Bots::spawn(Dependencies {
            store: self.store.clone(),
            workspaces: self.workspaces.clone(),
            projects: self.projects.clone(),
            executor: self.native.clone(),
            credentials,
            scopes: self.scopes.clone(),
        })
        .unwrap()
    }
}
/// Workspace registered by every [`Fixture`].
pub(crate) const WORKSPACE: &str = "wks_0123456789abcdef";
/// Second registered workspace, for isolation between Workspace Bots.
pub(crate) const OTHER_WORKSPACE: &str = "wks_fedcba9876543210";

pub(crate) fn setup(workspace: &str) -> Value {
    json!({"workspaceId":workspace,"execution":{"provider":"test","model":null,"modeId":null,"thinking":null}})
}
