use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use domain::agent_runtime::{PersistedAgentRuntimeRecord, TitleOrigin};
use domain::summary::{SummaryError, SummaryKind, SummaryRequest, SummarySelection};
use model::ErrorCode;
use serde_json::Value;
use tokio::task::JoinHandle;

use super::AgentManager;
use model::summary::SummaryGenerator;

#[derive(Debug)]
struct Pending {
    title: Option<String>,
    created_at: String,
    task: JoinHandle<Result<Value, SummaryError>>,
}

#[derive(Debug, Default)]
pub(super) struct Titles {
    pub(super) generator: Option<Arc<dyn SummaryGenerator>>,
    pending: BTreeMap<String, Pending>,
    attempted: BTreeSet<String>,
    next_scan: Option<std::time::Instant>,
}

impl Titles {
    pub(super) async fn close(&mut self) {
        for (_, pending) in std::mem::take(&mut self.pending) {
            pending.task.abort();
            let _ = pending.task.await;
        }
    }
}

impl Drop for Titles {
    fn drop(&mut self) {
        for pending in self.pending.values() {
            pending.task.abort();
        }
    }
}

impl AgentManager {
    /// Collect completed auxiliary titles and schedule eligible provisional records.
    /// Storage errors retain the fallback title; native generation never holds the command lane.
    pub(crate) async fn poll_generated_titles(&mut self) -> Result<(), ErrorCode> {
        let Some(generator) = self.generated_titles.generator.clone() else {
            return Ok(());
        };
        let completed: Vec<_> = self
            .generated_titles
            .pending
            .iter()
            .filter(|(_, pending)| pending.task.is_finished())
            .map(|(id, _)| id.clone())
            .collect();
        for id in completed {
            let pending = self
                .generated_titles
                .pending
                .remove(&id)
                .expect("selected pending title");
            let result = pending.task.await;
            if matches!(result, Ok(Err(SummaryError::Cancelled))) {
                self.generated_titles.attempted.remove(&id);
            }
            if let Ok(Ok(value)) = result
                && let Some(title) = value["title"].as_str()
            {
                self.registry
                    .update(&id, &|current| {
                        let mut next = current.clone();
                        if eligible(current)
                            && current.title == pending.title
                            && current.created_at == pending.created_at
                        {
                            next.title = Some(title.to_owned());
                            next.title_origin = Some(TitleOrigin::Generated);
                        }
                        next
                    })
                    .map_err(|_| ErrorCode::AgentIo)?;
            }
        }
        if self
            .generated_titles
            .next_scan
            .is_some_and(|at| std::time::Instant::now() < at)
        {
            return Ok(());
        }
        self.generated_titles.next_scan =
            Some(std::time::Instant::now() + std::time::Duration::from_secs(1));
        let Some(timeline) = &self.timeline else {
            return Ok(());
        };
        for record in self.registry.list().map_err(|_| ErrorCode::AgentIo)? {
            if self.generated_titles.pending.len() >= 2 {
                break;
            }
            if !eligible(&record) || self.generated_titles.attempted.contains(&record.id) {
                continue;
            }
            let Some(context) = timeline
                .first_user_text(&record.id)?
                .or_else(|| record.title.clone())
            else {
                continue;
            };
            let config = record.config.as_ref();
            let request = SummaryRequest {
                kind: SummaryKind::Title,
                cwd: record.cwd,
                context,
                selection: Some(SummarySelection {
                    provider: record.provider,
                    model: config.and_then(|config| config.model.clone()),
                    thinking_option_id: config.and_then(|config| config.thinking_option_id.clone()),
                }),
            };
            let generator = generator.clone();
            self.generated_titles.attempted.insert(record.id.clone());
            self.generated_titles.pending.insert(
                record.id,
                Pending {
                    title: record.title,
                    created_at: record.created_at,
                    task: tokio::spawn(async move { generator.generate(request).await }),
                },
            );
        }
        Ok(())
    }
}

fn eligible(record: &PersistedAgentRuntimeRecord) -> bool {
    !record.internal
        && record.archived_at.is_none()
        && record.title_origin == Some(TitleOrigin::Prompt)
}
