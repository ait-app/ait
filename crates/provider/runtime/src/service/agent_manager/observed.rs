//! Idempotent read-only registration of provider-verified native descendants.

use std::collections::{BTreeMap, VecDeque};

use domain::agent_runtime::observation::{ObservationHealth, ObservedAgent, ObservedStatus};
use domain::agent_runtime::registry::AgentRuntimeRegistry;
use domain::agent_runtime::{AgentRuntimeStatus, PersistedAgentRuntimeRecord};
use model::ErrorCode;
use sha2::{Digest, Sha256};

use super::{AgentManager, AgentManagerError};
use crate::storage::timeline::Timeline;
use provider_core::ports::controls::NativeSubagent;

struct RegistrationContext<'a> {
    registry: &'a dyn AgentRuntimeRegistry,
    timeline: &'a Timeline,
    root: &'a PersistedAgentRuntimeRecord,
    managed: Option<BTreeMap<String, Vec<PersistedAgentRuntimeRecord>>>,
}

/// Register an already ancestry-verified native tree in parent order without opening writers.
/// Returns one canonical host ID per native child, or validation/persistence conflicts.
pub(crate) fn register_children(
    registry: &dyn AgentRuntimeRegistry,
    timeline: &Timeline,
    root: &PersistedAgentRuntimeRecord,
    children: &[NativeSubagent],
) -> Result<BTreeMap<String, String>, ErrorCode> {
    let _registration = timeline
        .registration
        .lock()
        .map_err(|_| ErrorCode::AgentIo)?;
    let Some(current_root) = registry.get(&root.id).map_err(|_| ErrorCode::AgentIo)? else {
        return Ok(BTreeMap::new());
    };
    let root = &current_root;
    let handle = root
        .persistence
        .as_ref()
        .ok_or(ErrorCode::UnsupportedCapability)?;
    if root.observation.is_some() {
        return Err(ErrorCode::InvalidMessage);
    }
    let mut context = RegistrationContext {
        registry,
        timeline,
        root,
        managed: None,
    };
    let mut by_parent = BTreeMap::<&str, Vec<&NativeSubagent>>::new();
    for child in children {
        by_parent.entry(&child.parent_id).or_default().push(child);
    }
    let mut ids = BTreeMap::from([(handle.session_id.clone(), root.id.clone())]);
    let mut pending = VecDeque::from([handle.session_id.clone()]);
    while let Some(parent) = pending.pop_front() {
        for child in by_parent.remove(parent.as_str()).unwrap_or_default() {
            if ids.contains_key(&child.id) {
                return Err(ErrorCode::InvalidMessage);
            }
            let parent_id = ids.get(&parent).ok_or(ErrorCode::InvalidMessage)?;
            let id = register_child(&mut context, parent_id, child)?;
            ids.insert(child.id.clone(), id);
            pending.push_back(child.id.clone());
        }
    }
    if ids.len() != children.len() + 1 {
        return Err(ErrorCode::InvalidMessage);
    }
    ids.remove(&handle.session_id);
    Ok(ids)
}

fn register_child(
    context: &mut RegistrationContext<'_>,
    parent: &str,
    child: &NativeSubagent,
) -> Result<String, ErrorCode> {
    let registry = context.registry;
    let timeline = context.timeline;
    let root = context.root;
    let handle = root
        .persistence
        .as_ref()
        .ok_or(ErrorCode::UnsupportedCapability)?;
    let bound = timeline.subagent_agent(&root.id, &child.id)?;
    let id = if let Some(id) = &bound {
        id.clone()
    } else {
        let encoded = serde_json::to_vec(&(&root.provider, &handle.session_id, &child.id))
            .map_err(|_| ErrorCode::AgentIo)?;
        format!("observed_{:x}", Sha256::digest(encoded))
    };
    let previous = registry.get(&id).map_err(|_| ErrorCode::AgentIo)?;
    if previous.is_none()
        && bound.is_none()
        && let Some(id) = reuse_managed(context, parent, child)?
    {
        return Ok(id);
    }
    verify_binding(previous.as_ref(), root, parent, child, bound.is_some())?;
    if previous
        .as_ref()
        .is_some_and(|record| record.observation.is_none())
    {
        return Ok(id);
    }
    let mut source = source(root, parent, child)?;
    if source.status == ObservedStatus::Unknown
        && let Some(known) = previous
            .as_ref()
            .and_then(|record| record.observation.as_ref())
        && known.status.is_terminal()
    {
        source.status = known.status;
    }
    source.ready = bound.is_some()
        && previous
            .as_ref()
            .and_then(|record| record.observation.as_ref())
            .is_some_and(|known| known.ready);
    if previous
        .as_ref()
        .and_then(|record| record.observation.as_ref())
        .is_some_and(|known| known.ready && known.observed_at > source.observed_at)
    {
        return Ok(id);
    }
    timeline.store_subagent(&root.id, child)?;
    let record = project_record(root, child, previous.as_ref(), source.clone(), &id);
    if previous.as_ref() != Some(&record) {
        if previous.is_some() {
            registry
                .update(&id, &|current| {
                    project_record(root, child, Some(current), source.clone(), &id)
                })
                .map_err(|_| ErrorCode::AgentIo)?
                .ok_or(ErrorCode::AgentNotFound)?;
        } else {
            registry.upsert(&record).map_err(|_| ErrorCode::AgentIo)?;
        }
    }
    timeline.bind_subagent(&root.id, &child.id, &id, true)?;
    if !record
        .observation
        .as_ref()
        .is_some_and(|source| source.ready)
    {
        registry
            .update(&id, &|current| {
                let mut next = current.clone();
                if let Some(source) = &mut next.observation {
                    source.ready = true;
                }
                next
            })
            .map_err(|_| ErrorCode::AgentIo)?
            .ok_or(ErrorCode::AgentNotFound)?;
    }
    Ok(id)
}

fn project_record(
    root: &PersistedAgentRuntimeRecord,
    child: &NativeSubagent,
    previous: Option<&PersistedAgentRuntimeRecord>,
    mut source: ObservedAgent,
    id: &str,
) -> PersistedAgentRuntimeRecord {
    let mut record = match previous {
        Some(record) => record.clone(),
        None => new_record(root, child, id),
    };
    let prior = record.observation.as_ref();
    if record.title.is_none()
        || record.title_origin == Some(domain::agent_runtime::TitleOrigin::Prompt)
    {
        record.title = source
            .description
            .clone()
            .or_else(|| string(child, "title"));
    }
    let fresh_run = prior.is_some_and(|prior| prior.observed_at < source.observed_at)
        && source.status == ObservedStatus::Running;
    if fresh_run && root.archived_at.is_none() {
        record.archived_at = None;
    }
    if root.archived_at.is_some() {
        record.archived_at.clone_from(&root.archived_at);
        source.health = ObservationHealth::Unavailable;
    }
    record.last_status = runtime_status(&source);
    if chrono::DateTime::parse_from_rfc3339(&record.updated_at)
        .ok()
        .is_none_or(|current| {
            chrono::DateTime::parse_from_rfc3339(&source.observed_at)
                .is_ok_and(|observed| current < observed)
        })
    {
        record.updated_at.clone_from(&source.observed_at);
    }
    record.last_activity_at = Some(source.observed_at.clone());
    record.cwd.clone_from(&child.cwd);
    record.workspace_id.clone_from(&root.workspace_id);
    record.labels.insert(
        "paseo.parent-agent-id".to_owned(),
        source.parent_agent_id.clone(),
    );
    source.revision = prior.map_or(0, |prior| prior.revision);
    if prior != Some(&source) {
        source.revision = source.revision.saturating_add(1);
    }
    record.observation = Some(source);
    record
}

fn reuse_managed(
    context: &mut RegistrationContext<'_>,
    parent: &str,
    child: &NativeSubagent,
) -> Result<Option<String>, ErrorCode> {
    let registry = context.registry;
    let timeline = context.timeline;
    let root = context.root;
    if context.managed.is_none() {
        let mut index = BTreeMap::<String, Vec<PersistedAgentRuntimeRecord>>::new();
        for record in registry.list().map_err(|_| ErrorCode::AgentIo)? {
            if record.provider != root.provider {
                continue;
            }
            if let Some(handle) = &record.persistence {
                index
                    .entry(handle.session_id.clone())
                    .or_default()
                    .push(record);
            }
        }
        context.managed = Some(index);
    }
    let matches = context
        .managed
        .as_ref()
        .and_then(|index| index.get(&child.id))
        .map_or(&[][..], Vec::as_slice);
    if matches.len() > 1 {
        return Err(ErrorCode::IdempotencyConflict);
    }
    if let Some(managed) = matches.first() {
        timeline.store_subagent(&root.id, child)?;
        if managed
            .labels
            .get("paseo.parent-agent-id")
            .is_some_and(|known| known != parent)
        {
            return Err(ErrorCode::IdempotencyConflict);
        }
        registry
            .update(&managed.id, &|current| {
                let mut next = current.clone();
                next.labels
                    .insert("paseo.parent-agent-id".to_owned(), parent.to_owned());
                next
            })
            .map_err(|_| ErrorCode::AgentIo)?;
        timeline.bind_subagent(&root.id, &child.id, &managed.id, false)?;
        return Ok(Some(managed.id.clone()));
    }
    Ok(None)
}

fn verify_binding(
    previous: Option<&PersistedAgentRuntimeRecord>,
    root: &PersistedAgentRuntimeRecord,
    parent: &str,
    child: &NativeSubagent,
    bound: bool,
) -> Result<(), ErrorCode> {
    let handle = root
        .persistence
        .as_ref()
        .ok_or(ErrorCode::UnsupportedCapability)?;
    if let Some(record) = previous {
        let Some(source) = &record.observation else {
            if !bound {
                return Err(ErrorCode::IdempotencyConflict);
            }
            return Ok(());
        };
        if record.provider != root.provider
            || source.root_agent_id != root.id
            || source.root_session_id != handle.session_id
            || source.parent_agent_id != parent
            || source.native_id != child.id
            || source.parent_native_id != child.parent_id
        {
            return Err(ErrorCode::IdempotencyConflict);
        }
    }
    Ok(())
}

fn string(child: &NativeSubagent, field: &str) -> Option<String> {
    child.descriptor[field]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
}

fn source(
    root: &PersistedAgentRuntimeRecord,
    parent: &str,
    child: &NativeSubagent,
) -> Result<ObservedAgent, ErrorCode> {
    let observed_at = string(child, "updatedAt").ok_or(ErrorCode::InvalidMessage)?;
    let observed_at = chrono::DateTime::parse_from_rfc3339(&observed_at)
        .map_err(|_| ErrorCode::InvalidMessage)?
        .with_timezone(&chrono::Utc)
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    Ok(ObservedAgent {
        revision: 0,
        root_agent_id: root.id.clone(),
        root_session_id: root
            .persistence
            .as_ref()
            .ok_or(ErrorCode::InvalidMessage)?
            .session_id
            .clone(),
        parent_agent_id: parent.to_owned(),
        native_id: child.id.clone(),
        parent_native_id: child.parent_id.clone(),
        persistence: child.persistence.clone(),
        ready: false,
        status: match child.descriptor["status"].as_str() {
            Some("completed") if child.descriptor["outcomeKnown"] == false => {
                ObservedStatus::Unknown
            }
            Some("running") => ObservedStatus::Running,
            Some("completed") => ObservedStatus::Completed,
            Some("failed") => ObservedStatus::Failed,
            Some("canceled") => ObservedStatus::Canceled,
            _ => ObservedStatus::Unknown,
        },
        health: if child.descriptor["observationStale"] == true {
            ObservationHealth::Stale
        } else {
            ObservationHealth::Current
        },
        observed_at,
        description: string(child, "description"),
        subtitle: string(child, "subtitle").or_else(|| string(child, "title")),
    })
}

fn new_record(
    root: &PersistedAgentRuntimeRecord,
    child: &NativeSubagent,
    id: &str,
) -> PersistedAgentRuntimeRecord {
    PersistedAgentRuntimeRecord {
        id: id.to_owned(),
        provider: root.provider.clone(),
        cwd: child.cwd.clone(),
        workspace_id: root.workspace_id.clone(),
        created_at: string(child, "createdAt").unwrap_or_else(|| root.created_at.clone()),
        updated_at: root.updated_at.clone(),
        last_activity_at: None,
        last_user_message_at: None,
        title: None,
        title_origin: Some(domain::agent_runtime::TitleOrigin::Prompt),
        labels: BTreeMap::new(),
        last_status: AgentRuntimeStatus::Closed,
        last_mode_id: None,
        config: None,
        runtime_info: None,
        features: Vec::new(),
        persistence: None,
        last_error: None,
        requires_attention: false,
        attention_reason: None,
        attention_timestamp: None,
        internal: root.internal,
        archived_at: None,
        owner: None,
        observation: None,
    }
}

/// Resolve the original child scope without changing its generation or sequence identities.
pub(crate) fn scope(source: &ObservedAgent) -> String {
    format!("subagent:{}:{}", source.root_agent_id, source.native_id)
}

/// Project source health and confirmed execution state into the existing lifecycle protocol.
pub(crate) const fn runtime_status(source: &ObservedAgent) -> AgentRuntimeStatus {
    if !matches!(source.health, ObservationHealth::Current) {
        return AgentRuntimeStatus::Closed;
    }
    match source.status {
        ObservedStatus::Running => AgentRuntimeStatus::Running,
        ObservedStatus::Failed => AgentRuntimeStatus::Error,
        ObservedStatus::Completed | ObservedStatus::Canceled => AgentRuntimeStatus::Idle,
        ObservedStatus::Unknown => AgentRuntimeStatus::Closed,
    }
}

impl AgentManager {
    /// Read a child through its verified root without opening or resuming an interactive writer.
    /// Returns a native read error only when no durable child history can be displayed.
    pub(crate) async fn load_observed(&self, id: &str) -> Result<(), ErrorCode> {
        let record = self
            .registry
            .get(id)
            .map_err(|_| ErrorCode::AgentIo)?
            .ok_or(ErrorCode::AgentNotFound)?;
        let source = record
            .observation
            .as_ref()
            .ok_or(ErrorCode::InvalidMessage)?;
        let timeline = self
            .timeline
            .as_ref()
            .ok_or(ErrorCode::UnsupportedCapability)?;
        let scope = scope(source);
        let root = self
            .registry
            .get(&source.root_agent_id)
            .map_err(|_| ErrorCode::AgentIo)?;
        let available = root.as_ref().is_some_and(|root| {
            root.archived_at.is_none()
                && root
                    .persistence
                    .as_ref()
                    .is_some_and(|handle| handle.session_id == source.root_session_id)
        });
        if !available {
            self.registry
                .update(id, &|current| {
                    let mut next = current.clone();
                    if let Some(source) = &mut next.observation {
                        if source.health != ObservationHealth::Unavailable {
                            source.revision = source.revision.saturating_add(1);
                        }
                        source.health = ObservationHealth::Unavailable;
                    }
                    next.last_status = AgentRuntimeStatus::Closed;
                    next
                })
                .map_err(|_| ErrorCode::AgentIo)?;
            self.mark_history_loaded(id);
            return Ok(());
        }
        let _ = self.subagents(&source.root_agent_id).await;
        let child = NativeSubagent {
            persistence: source.persistence.clone(),
            id: source.native_id.clone(),
            parent_id: source.parent_native_id.clone(),
            cwd: record.cwd.clone(),
            descriptor: serde_json::Value::Null,
        };
        match self.child_history(&source.root_agent_id, &child).await {
            Ok(history) => {
                if history.active || self.live_child(&source.root_agent_id, &source.native_id) {
                    timeline.append(&scope, &record.provider, &history.entries)?;
                } else {
                    timeline.reconcile(&scope, &record.provider, &history.entries)?;
                }
            }
            Err(_)
                if timeline.has_entries(&scope)?
                    || self.live_child(&source.root_agent_id, &source.native_id) => {}
            Err(error) => return Err(error),
        }
        self.mark_history_loaded(id);
        Ok(())
    }

    /// Reject independent execution of an observed identity before native or input side effects.
    /// Returns storage or read-only errors; callers retain their existing missing-record semantics.
    pub(crate) fn require_managed(&self, id: &str) -> Result<(), AgentManagerError> {
        let record = self.registry.get(id).map_err(super::map_registry)?;
        if record.is_some_and(|record| record.observation.is_some()) {
            return Err(AgentManagerError::Busy);
        }
        Ok(())
    }

    /// Restore durable child registrations and mark cached state stale without opening writers.
    /// Each registry write replaces the whole file, so records already in their recovered state
    /// are left untouched and repeated restarts write nothing for them.
    /// Returns persistence failures; source confirmation happens on later native reads.
    pub(crate) fn recover_observed(&self) -> Result<(), AgentManagerError> {
        let Some(timeline) = &self.timeline else {
            return Ok(());
        };
        let records = self.registry.snapshot().map_err(super::map_registry)?;
        for root in records
            .iter()
            .filter(|record| record.observation.is_none() && record.persistence.is_some())
        {
            let native = &root
                .persistence
                .as_ref()
                .ok_or(AgentManagerError::InvalidRequest)?
                .session_id;
            let mut children = timeline
                .subagents(&root.id)
                .map_err(|_| AgentManagerError::Registry)?;
            // Cached children await native confirmation, so project them as stale directly.
            for child in &mut children {
                child.descriptor["observationStale"] = serde_json::Value::Bool(true);
            }
            let Ok(children) = super::controls::descendants(native, children) else {
                tracing::warn!(agent_id = %root.id, "Cached native child ancestry could not be normalized");
                continue;
            };
            match register_children(self.registry.as_ref(), timeline, root, &children) {
                Ok(_) => {}
                Err(ErrorCode::InvalidMessage | ErrorCode::IdempotencyConflict) => {
                    tracing::warn!(agent_id = %root.id, "Cached native child identity could not be normalized");
                }
                Err(_) => return Err(AgentManagerError::Registry),
            }
        }
        let recovered = self.registry.snapshot().map_err(super::map_registry)?;
        for record in recovered.iter() {
            let Some(source) = &record.observation else {
                continue;
            };
            if source.health != ObservationHealth::Current
                && record.last_status == AgentRuntimeStatus::Closed
            {
                continue;
            }
            self.registry
                .update(&record.id, &|current| {
                    let mut next = current.clone();
                    if let Some(source) = &mut next.observation
                        && source.health == ObservationHealth::Current
                    {
                        source.revision = source.revision.saturating_add(1);
                        source.health = ObservationHealth::Stale;
                    }
                    next.last_status = AgentRuntimeStatus::Closed;
                    next
                })
                .map_err(super::map_registry)?;
        }
        Ok(())
    }
}
