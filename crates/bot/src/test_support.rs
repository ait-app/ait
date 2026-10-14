use std::collections::BTreeMap;
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
}
impl Executor for Native {
    fn execute(
        &self,
        owner: SessionOwner,
        action: Action,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Value, ErrorCode>> + Send + '_>> {
        Box::pin(async move {
            self.calls.lock().unwrap().push((owner, action.clone()));
            Ok(match action {
                Action::Create { .. } | Action::Close { .. } | Action::Cancel { .. } => {
                    json!({"ok":true})
                }
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
                    if self.approval.load(Ordering::SeqCst) {
                        json!({"status":"permission","final":{"pendingPermissions":[{"id":"approval-1","options":["allow","deny"]}]}})
                    } else if self.hold.load(Ordering::SeqCst) {
                        json!({"status":"running"})
                    } else {
                        json!({"status":"idle","lastMessage":format!("reply:{}",self.messages.lock().unwrap().get(&agent_id).unwrap())})
                    }
                }
            })
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

pub(crate) struct Fixture {
    pub(crate) root: tempfile::TempDir,
    pub(crate) store: Arc<FileStore>,
    pub(crate) workspaces: Arc<FileBackedWorkspaceRegistry>,
    pub(crate) projects: Arc<FileBackedProjectRegistry>,
    pub(crate) native: Arc<Native>,
}
impl Fixture {
    pub(crate) fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().to_str().unwrap();
        let projects = Arc::new(FileBackedProjectRegistry::new(
            root.path().join("projects.json"),
        ));
        projects.upsert(&serde_json::from_value(json!({"projectId":"project-1","rootPath":cwd,"kind":"non_git","displayName":"project", "createdAt":"2026-10-12T00:00:00Z","updatedAt":"2026-10-12T00:00:00Z","archivedAt":null})).unwrap()).unwrap();
        let workspaces = Arc::new(FileBackedWorkspaceRegistry::new(
            root.path().join("workspaces.json"),
        ));
        workspaces.upsert(&serde_json::from_value(json!({"workspaceId":"wks_0123456789abcdef","projectId":"project-1","cwd":cwd,"kind":"local_checkout","displayName":"workspace", "createdAt":"2026-10-12T00:00:00Z","updatedAt":"2026-10-12T00:00:00Z","archivedAt":null})).unwrap(), domain::workspace::registry::WorkspaceMutationContext::default()).unwrap();
        Self {
            store: Arc::new(FileStore::new(root.path().join("bots"))),
            root,
            workspaces,
            projects,
            native: Arc::default(),
        }
    }
    pub(crate) fn state(&self) -> State {
        State::load(
            self.store.clone(),
            self.workspaces.clone(),
            self.projects.clone(),
        )
        .unwrap()
    }
    pub(crate) fn bots(&self) -> Bots {
        Bots::spawn(Dependencies {
            store: self.store.clone(),
            workspaces: self.workspaces.clone(),
            projects: self.projects.clone(),
            executor: self.native.clone(),
            credentials: Arc::new(NoCredentials),
        })
        .unwrap()
    }
}
pub(crate) fn creation(id: domain::BotId) -> Value {
    json!({"botId":id,"workspaceId":"wks_0123456789abcdef","name":"assistant","instructions":"help", "execution":{"provider":"test","model":null,"modeId":null,"thinking":null}})
}
