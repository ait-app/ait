//! Workspace status priority and entry-time history, following Paseo `WorkspaceDirectory`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use model::workspace::activity::WorkspaceStateBucket;
use model::workspace::attention::{WorkspaceActivity, WorkspaceActivitySource};
use model::workspace::records::PersistedWorkspaceRecord;

use super::{Directory, DirectoryError};

/// Aggregated status and its stable entry time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Status {
    /// Highest-priority active bucket.
    pub(crate) bucket: WorkspaceStateBucket,
    /// Timestamp of the latest bucket transition.
    pub(crate) entered_at: String,
}

#[derive(Debug, Clone, Default)]
pub(super) struct ActivityProjection {
    sources: Vec<Arc<dyn WorkspaceActivitySource>>,
    history: Arc<Mutex<BTreeMap<String, Status>>>,
}

impl ActivityProjection {
    pub(super) fn add(&mut self, source: Arc<dyn WorkspaceActivitySource>) {
        self.sources.push(source);
    }
}

impl Directory {
    /// Aggregate active Workspace contributions and preserve entry times between snapshots.
    ///
    /// # Errors
    /// Returns a registry failure when the activity adapter cannot capture its source.
    pub(crate) fn workspace_statuses(
        &self,
        workspaces: &[PersistedWorkspaceRecord],
        now: &str,
    ) -> Result<BTreeMap<String, Status>, DirectoryError> {
        let mut contributions = Vec::new();
        for source in &self.activity.sources {
            contributions.extend(source.snapshot().map_err(|_| DirectoryError::Registry)?);
        }
        let mut history = self
            .activity
            .history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let current = project(workspaces, &contributions, &history, now);
        history.clone_from(&current);
        Ok(current)
    }
}

fn project(
    workspaces: &[PersistedWorkspaceRecord],
    contributions: &[WorkspaceActivity],
    history: &BTreeMap<String, Status>,
    now: &str,
) -> BTreeMap<String, Status> {
    let mut winners: BTreeMap<&str, (WorkspaceStateBucket, Option<&str>)> = BTreeMap::new();
    for contribution in contributions {
        let changed_at = contribution
            .changed_at
            .as_deref()
            .filter(|time| chrono::DateTime::parse_from_rfc3339(time).is_ok());
        let winner = winners
            .entry(&contribution.workspace_id)
            .or_insert((contribution.bucket, changed_at));
        if contribution.bucket.priority() < winner.0.priority() {
            *winner = (contribution.bucket, changed_at);
        } else if contribution.bucket == winner.0 {
            winner.1 = winner.1.max(changed_at);
        }
    }
    workspaces
        .iter()
        .map(|workspace| {
            let winner = winners.get(workspace.workspace_id.as_str());
            let bucket = winner.map_or(WorkspaceStateBucket::Done, |winner| winner.0);
            let previous = history.get(&workspace.workspace_id);
            let entered_at = match previous {
                Some(previous) if previous.bucket == bucket => previous.entered_at.clone(),
                Some(_) => now.to_owned(),
                None => winner
                    .map_or(workspace.created_at.as_str(), |winner| {
                        winner.1.unwrap_or(now)
                    })
                    .to_owned(),
            };
            (
                workspace.workspace_id.clone(),
                Status { bucket, entered_at },
            )
        })
        .collect()
}
