//! Bounded first-prompt naming, with durable eligibility and compare-before-write protection.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::ports::generation::{SummarySource, WorkspaceBranchNamer};
use crate::ports::registry::WorkspaceRegistry;
use model::summary::{SummaryKind, SummaryRequest, SummarySelection};

pub use model::workspace::naming::first_agent_source;

/// Shared background coordinator for directory titles and managed placeholder branches.
#[derive(Debug, Clone)]
pub struct WorkspaceNames {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    registry: Arc<dyn WorkspaceRegistry>,
    generator: Arc<dyn SummarySource>,
    branches: Arc<dyn WorkspaceBranchNamer>,
    handle: tokio::runtime::Handle,
    pending: Mutex<BTreeSet<String>>,
    cancel: CancellationToken,
    tasks: TaskTracker,
}

impl WorkspaceNames {
    /// Compose shared adapters on a Tokio runtime. At most 32 distinct workspaces are queued.
    /// `branches` applies Git ownership checks independently of the generated wording.
    #[must_use]
    pub fn new(
        registry: Arc<dyn WorkspaceRegistry>,
        generator: Arc<dyn SummarySource>,
        branches: Arc<dyn WorkspaceBranchNamer>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                registry,
                generator,
                branches,
                handle: tokio::runtime::Handle::current(),
                pending: Mutex::new(BTreeSet::new()),
                cancel: CancellationToken::new(),
                tasks: TaskTracker::new(),
            }),
        }
    }

    /// Queue nonempty first-prompt source material without delaying creation or foreground turns.
    /// Duplicate, oversized, draining, and full-queue requests are ignored; explicit titles stay intact.
    pub fn schedule(&self, id: String, context: String, selection: Option<SummarySelection>) {
        if context.trim().is_empty() || context.len() > 1024 * 1024 {
            return;
        }
        let mut pending = self
            .inner
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.inner.cancel.is_cancelled() || pending.len() >= 32 || !pending.insert(id.clone()) {
            return;
        }
        let inner = self.inner.clone();
        self.inner.tasks.spawn_on(
            async move {
                tokio::select! {
                    biased;
                    () = inner.cancel.cancelled() => {},
                    () = generate(&inner, &id, context, selection) => {},
                }
                inner
                    .pending
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(&id);
            },
            &self.inner.handle,
        );
    }

    /// Cancel queued model operations and close background admission during server drain.
    pub fn shutdown(&self) {
        let _admission = self
            .inner
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.inner.cancel.cancel();
        self.inner.tasks.close();
    }

    /// Wait for admitted naming tasks, including already-started blocking registry writes.
    pub async fn wait_closed(&self) {
        self.inner.tasks.wait().await;
    }
}

async fn generate(inner: &Inner, id: &str, context: String, selection: Option<SummarySelection>) {
    let registry = inner.registry.clone();
    let identity = id.to_owned();
    let Ok(Ok(Some(before))) = tokio::task::spawn_blocking(move || registry.get(&identity)).await
    else {
        return;
    };
    if before.auto_name.is_none() || before.archived_at.is_some() {
        return;
    }
    let Ok(value) = inner
        .generator
        .generate(SummaryRequest {
            kind: SummaryKind::BranchName,
            cwd: before.cwd.clone(),
            context,
            selection,
        })
        .await
    else {
        return;
    };
    let Some(title) = value["title"].as_str().map(str::to_owned) else {
        return;
    };
    let registry = inner.registry.clone();
    let branches = inner.branches.clone();
    let cancel = inner.cancel.clone();
    // Keep blocking effects tracked even when the surrounding generation future is cancelled.
    let tracking = inner.tasks.token();
    let _ = tokio::task::spawn_blocking(move || {
        let _tracking = tracking;
        if cancel.is_cancelled() {
            return;
        }
        let _ = registry.update(&before.workspace_id, &|current| {
            let mut next = current.clone();
            if current.archived_at.is_some()
                || current.auto_name != before.auto_name
                || current.title != before.title
                || current.cwd != before.cwd
                || current.created_at != before.created_at
            {
                return next;
            }
            next.title = Some(title.clone());
            next.auto_name = None;
            if let Some(expected) = before
                .auto_name
                .as_ref()
                .and_then(|name| name.placeholder_branch.as_deref())
                && current.branch.as_deref() == Some(expected)
                && let Some(desired) = value.get("branch").and_then(Value::as_str)
                && let Some(branch) = branches.rename(&current.cwd, expected, desired)
            {
                next.branch = Some(branch);
            }
            next.updated_at =
                chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
            next
        });
    })
    .await;
}

#[cfg(test)]
mod tests;
