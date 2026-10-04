//! Shared fixtures for unit tests.

use serde_json::Map;

use crate::wire::{BonsaiRef, Dispatch, ProjectRef, Requester, Task};

/// A minimal valid dispatch for `run_id`.
pub(crate) fn dispatch(run_id: &str) -> Dispatch {
    Dispatch {
        run_id: run_id.to_owned(),
        space_id: "sandbox".to_owned(),
        task: Task {
            path: "1_Projects/Sandbox/Sandbox.md".to_owned(),
            line: 13,
            text: "- [ ] list the files".to_owned(),
            heading: Some("Next Steps".to_owned()),
            context: "- [ ] list the files".to_owned(),
        },
        project: ProjectRef {
            id: "prj_0123456789abcdef".to_owned(),
        },
        provider: None,
        model: None,
        instruction: String::new(),
        settings: Map::new(),
        wrapup: "wrap up".to_owned(),
        bonsai: BonsaiRef {
            mcp_url: "http://localhost:8860/mcp".to_owned(),
        },
        requested_by: Requester {
            id: "github:900002".to_owned(),
            login: Some("sandbox-member".to_owned()),
            owner: false,
        },
        session: "bonsai.session/1".to_owned(),
    }
}

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use model::ErrorCode;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::ports::{
    Backfill, Backlog, BoxFuture, Executor, Host, HostEvent, Observation, Observer, PortError,
    Project, Projects, Workspace,
};

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Scripted Agent RPC that records every call.
#[derive(Debug, Default)]
pub(crate) struct MockExecutor {
    calls: Mutex<Vec<(String, Value)>>,
    queued: Mutex<HashMap<String, VecDeque<Result<Value, ErrorCode>>>>,
    fallback: Mutex<HashMap<String, Result<Value, ErrorCode>>>,
    gates: Mutex<HashMap<String, Arc<tokio::sync::Semaphore>>>,
}

impl MockExecutor {
    /// Answer the next call of `method` with `result`.
    pub(crate) fn respond(&self, method: &str, result: Result<Value, ErrorCode>) {
        lock(&self.queued)
            .entry(method.to_owned())
            .or_default()
            .push_back(result);
    }

    /// Answer every call of `method` without a queued answer with `result`.
    pub(crate) fn always(&self, method: &str, result: Result<Value, ErrorCode>) {
        lock(&self.fallback).insert(method.to_owned(), result);
    }

    /// Make calls of `method` wait until [`MockExecutor::release`].
    pub(crate) fn hold(&self, method: &str) -> Arc<tokio::sync::Semaphore> {
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        lock(&self.gates).insert(method.to_owned(), Arc::clone(&gate));
        gate
    }

    /// Every call so far, in order.
    pub(crate) fn calls(&self) -> Vec<(String, Value)> {
        lock(&self.calls).clone()
    }

    /// Calls of one method, in order.
    pub(crate) fn calls_of(&self, method: &str) -> Vec<Value> {
        self.calls()
            .into_iter()
            .filter(|(name, _)| name == method)
            .map(|(_, params)| params)
            .collect()
    }
}

impl Executor for MockExecutor {
    fn execute(
        &self,
        method: &'static str,
        params: Value,
    ) -> BoxFuture<'_, Result<Value, ErrorCode>> {
        Box::pin(async move {
            lock(&self.calls).push((method.to_owned(), params));
            let gate = lock(&self.gates).get(method).cloned();
            if let Some(gate) = gate {
                let _permit = gate.acquire().await;
            }
            let queued = lock(&self.queued)
                .get_mut(method)
                .and_then(VecDeque::pop_front);
            queued.unwrap_or_else(|| {
                lock(&self.fallback)
                    .get(method)
                    .cloned()
                    .unwrap_or(Err(ErrorCode::MethodNotFound))
            })
        })
    }
}

/// Observer whose event channels the test feeds.
#[derive(Debug, Default)]
pub(crate) struct MockObserver {
    senders: Mutex<HashMap<String, mpsc::UnboundedSender<HostEvent>>>,
    observed: Mutex<Vec<String>>,
    failures: Mutex<u32>,
}

impl MockObserver {
    /// Deliver an event to the current observation of `agent_id`.
    pub(crate) fn emit(&self, agent_id: &str, method: &str, params: Value) -> bool {
        lock(&self.senders).get(agent_id).is_some_and(|sender| {
            sender
                .send(HostEvent {
                    method: method.to_owned(),
                    params,
                })
                .is_ok()
        })
    }

    /// Close the current observation of `agent_id`, as an overflow would.
    pub(crate) fn close(&self, agent_id: &str) {
        lock(&self.senders).remove(agent_id);
    }

    /// Agents observed so far, in order.
    pub(crate) fn observed(&self) -> Vec<String> {
        lock(&self.observed).clone()
    }

    /// Refuse the next `count` observations, as a host whose event hub is unavailable would.
    pub(crate) fn fail(&self, count: u32) {
        *lock(&self.failures) = count;
    }
}

impl Observer for MockObserver {
    fn observe(&self, agent_id: &str) -> Result<Observation, PortError> {
        {
            let mut failures = lock(&self.failures);
            if *failures > 0 {
                *failures -= 1;
                return Err(PortError::Closed);
            }
        }
        let (sender, receiver) = mpsc::unbounded_channel();
        lock(&self.senders).insert(agent_id.to_owned(), sender);
        lock(&self.observed).push(agent_id.to_owned());
        Ok(Observation::new(receiver, Box::new(|| Ok(()))))
    }
}

/// Timeline reads from a map the test fills.
#[derive(Debug, Default)]
pub(crate) struct MockBackfill {
    backlogs: Mutex<HashMap<String, Backlog>>,
}

impl MockBackfill {
    /// Set the generation `read` returns for `agent_id`.
    pub(crate) fn set(&self, agent_id: &str, backlog: Backlog) {
        lock(&self.backlogs).insert(agent_id.to_owned(), backlog);
    }
}

impl Backfill for MockBackfill {
    fn read(&self, agent_id: &str) -> BoxFuture<'_, Result<Backlog, PortError>> {
        let backlog = lock(&self.backlogs)
            .get(agent_id)
            .cloned()
            .unwrap_or_default();
        Box::pin(async move { Ok(backlog) })
    }
}

/// A fixed project list with one workspace per project.
#[derive(Debug, Default)]
pub(crate) struct MockProjects {
    pub(crate) projects: Mutex<Vec<Project>>,
    lists: AtomicUsize,
    gate: Mutex<Option<Arc<tokio::sync::Semaphore>>>,
}

impl MockProjects {
    /// One project with ID `prj_0123456789abcdef` rooted at `/tmp/repo`.
    pub(crate) fn single() -> Self {
        Self {
            projects: Mutex::new(vec![Project {
                id: "prj_0123456789abcdef".to_owned(),
                name: "repo".to_owned(),
                root: "/tmp/repo".to_owned(),
                remote_url: Some("git@github.com:me/Repo.git".to_owned()),
                branch: Some("main".to_owned()),
            }]),
            ..Self::default()
        }
    }

    /// How many times the project list was read.
    pub(crate) fn lists(&self) -> usize {
        self.lists.load(Ordering::SeqCst)
    }

    /// Make later `list` calls wait for a permit on the returned gate.
    pub(crate) fn hold(&self) -> Arc<tokio::sync::Semaphore> {
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        *lock(&self.gate) = Some(Arc::clone(&gate));
        gate
    }
}

impl Projects for MockProjects {
    fn list(&self) -> BoxFuture<'_, Result<Vec<Project>, PortError>> {
        self.lists.fetch_add(1, Ordering::SeqCst);
        let projects = lock(&self.projects).clone();
        let gate = lock(&self.gate).clone();
        Box::pin(async move {
            if let Some(gate) = gate {
                let _permit = gate.acquire().await;
            }
            Ok(projects)
        })
    }

    fn open_workspace(
        &self,
        project_id: &str,
    ) -> BoxFuture<'_, Result<Option<Workspace>, PortError>> {
        let found = lock(&self.projects)
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| Workspace {
                workspace_id: format!("wks_{}", &project.id[4..]),
                cwd: project.root.clone(),
            });
        Box::pin(async move { Ok(found) })
    }
}

/// Mocks wired as host ports, with handles kept for the test.
pub(crate) struct MockHost {
    pub(crate) executor: Arc<MockExecutor>,
    pub(crate) observer: Arc<MockObserver>,
    pub(crate) backfill: Arc<MockBackfill>,
    pub(crate) projects: Arc<MockProjects>,
}

impl MockHost {
    /// A host with one project and Claude available.
    pub(crate) fn new() -> Self {
        let executor = Arc::new(MockExecutor::default());
        executor.always(
            "provider.available.list.request",
            Ok(serde_json::json!({"providers": [
                {"provider": "claude", "available": true, "error": null},
                {"provider": "codex", "available": false, "error": "Provider executable is unavailable"}]})),
        );
        executor.always(
            "provider.models.list.request",
            Ok(serde_json::json!({"provider": "claude", "models": [
                {"id": "default", "label": "Default (recommended)"},
                {"id": "sonnet", "label": "Sonnet"}]})),
        );
        Self {
            executor,
            observer: Arc::new(MockObserver::default()),
            backfill: Arc::new(MockBackfill::default()),
            projects: Arc::new(MockProjects::single()),
        }
    }

    /// The ports for the adapter.
    pub(crate) fn host(&self) -> Host {
        Host {
            executor: self.executor.clone(),
            observer: self.observer.clone(),
            backfill: self.backfill.clone(),
            projects: self.projects.clone(),
        }
    }
}
