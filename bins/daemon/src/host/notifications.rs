//! Feed Workspace events into the Workspace Bot's notification inbox (ADR-271).
//!
//! Agent and terminal attention arrive through the unfiltered committed-attention tap of the
//! session events; schedule run results arrive from the schedule runner's settlement hook. Each
//! event becomes one [`Notification`] with a deterministic id, offered to [`Bots::notify`],
//! which drops it without side effects when no active Bot is concerned.

use std::sync::Arc;

use bot::service::Bots;
use chrono::{DateTime, TimeDelta, Utc};
use domain::agent_runtime::registry::AgentRuntimeRegistry;
use domain::agent_runtime::{AgentAttentionReason, PersistedAgentRuntimeRecord};
use domain::bot::notification::{Notification, NotificationKind};
use domain::bot::{BOT_CONVERSATION_LABEL, BOT_SCOPE_LABEL, BOT_TICKET_LABEL};
use domain::schedule::{Run, RunStatus, Schedule, Target};
use domain::session::protocol::SessionEventKind;
use model::ErrorCode;
use model::session::SessionEvents;
use serde_json::Value;
use tokio::sync::broadcast;

/// How far back the start-up and lag rescans look for Agents that still require attention.
///
/// Attention older than this is never replayed, so an Agent left waiting for days does not wake
/// the Bot again after every daemon restart; the Bot's own dedupe covers the window.
const RESCAN_WINDOW: TimeDelta = TimeDelta::minutes(15);
/// Label on Agents created by a schedule run; their run result is the notification instead,
/// including for permission requests, which fail the run.
const SCHEDULE_RUN_LABEL: &str = "paseo.schedule-run";
/// Largest schedule output or error copied into a notification; the Bot keeps no more.
const SUMMARY_BYTES: usize = 1024;

/// Offers notifications to the Bot service on the daemon runtime, never blocking the caller.
#[derive(Debug, Clone)]
pub(super) struct Notifier {
    bots: Bots,
    runtime: tokio::runtime::Handle,
}

impl Notifier {
    /// A notifier on the current runtime; `None` outside one.
    pub(super) fn current(bots: Bots) -> Option<Self> {
        tokio::runtime::Handle::try_current()
            .ok()
            .map(|runtime| Self { bots, runtime })
    }

    /// Offer `notification` in the background; the Bot service's shutdown waits for it.
    pub(super) fn offer(&self, notification: Notification) {
        self.bots.offer(&self.runtime, notification);
    }

    /// Forward the committed Agent and terminal attention of `events` until the Bots stop,
    /// reading each Agent's current record from `agents`.
    ///
    /// Subscribes before the start-up rescan, so attention raised meanwhile is not missed.
    pub(super) fn forward_attention(
        &self,
        events: &SessionEvents,
        agents: Arc<dyn AgentRuntimeRegistry>,
    ) {
        let receiver = events.subscribe_committed_attention();
        self.runtime
            .spawn(forward(self.bots.clone(), receiver, agents));
    }
}

/// Offer `notification`, logging failures other than a stopping service.
async fn deliver(bots: &Bots, notification: Notification) {
    match bots.notify(notification).await {
        Ok(_) | Err(ErrorCode::ServerDraining) => {}
        Err(error) => tracing::warn!(?error, "Bot notification was not admitted"),
    }
}

/// Rescan, then turn every committed attention event into a notification until `bots` stops;
/// a lagging receiver rescans instead of replaying the lost events.
async fn forward(
    bots: Bots,
    mut events: broadcast::Receiver<(SessionEventKind, Value)>,
    agents: Arc<dyn AgentRuntimeRegistry>,
) {
    rescan(&bots, &agents).await;
    loop {
        let event = tokio::select! {
            biased;
            () = bots.stopped() => break,
            event = events.recv() => event,
        };
        match event {
            Ok((kind, payload)) => {
                if let Some(notification) = attention(&agents, kind, &payload).await {
                    deliver(&bots, notification).await;
                }
            }
            Err(broadcast::error::RecvError::Lagged(count)) => {
                tracing::warn!(count, "Bot attention feed lagged; rescanning Agents");
                rescan(&bots, &agents).await;
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

/// The notification for one committed attention event, built from the Agent's current record.
async fn attention(
    agents: &Arc<dyn AgentRuntimeRegistry>,
    kind: SessionEventKind,
    payload: &Value,
) -> Option<Notification> {
    if kind == SessionEventKind::TerminalAttention {
        return terminal_notification(payload, Utc::now());
    }
    if kind != SessionEventKind::AgentAttention {
        return None;
    }
    let reason = match payload["reason"].as_str()? {
        "finished" => AgentAttentionReason::Finished,
        "error" => AgentAttentionReason::Error,
        "permission" => AgentAttentionReason::Permission,
        _ => return None,
    };
    let timestamp = payload["timestamp"].as_str()?;
    let id = payload["agentId"].as_str()?.to_owned();
    let registry = agents.clone();
    let Ok(Ok(record)) = tokio::task::spawn_blocking(move || registry.get(&id)).await else {
        tracing::warn!("cannot read the Agent of an attention event");
        return None;
    };
    agent_notification(&record?, reason, timestamp)
}

/// Offer every Agent that currently requires attention raised within [`RESCAN_WINDOW`].
async fn rescan(bots: &Bots, agents: &Arc<dyn AgentRuntimeRegistry>) {
    let registry = agents.clone();
    let Ok(Ok(records)) = tokio::task::spawn_blocking(move || registry.snapshot()).await else {
        tracing::warn!("cannot read Agents to rescan their attention");
        return;
    };
    for notification in recent_attention(&records, Utc::now()) {
        deliver(bots, notification).await;
    }
}

/// Notifications for `records` that require attention raised no earlier than
/// [`RESCAN_WINDOW`] before `now`.
fn recent_attention(
    records: &[Arc<PersistedAgentRuntimeRecord>],
    now: DateTime<Utc>,
) -> Vec<Notification> {
    let since = now - RESCAN_WINDOW;
    records
        .iter()
        .filter(|record| record.requires_attention)
        .filter_map(|record| {
            let reason = record.attention_reason?;
            let timestamp = record.attention_timestamp.as_deref()?;
            agent_notification(record, reason, timestamp)
        })
        .filter(|notification| notification.at >= since)
        .collect()
}

/// The notification for `record`'s attention `reason` raised at `timestamp`.
///
/// Returns `None` for internal sessions (the Bot's own), archived Agents, attention superseded
/// by a newer one, Agents without a Workspace, an unparsable timestamp, and any attention of an
/// Agent a schedule run created: its run result notifies instead, and a permission request
/// fails that run, whose Agent nobody can approve any more.
/// Attention a client already cleared still notifies: the Bot needs it regardless.
fn agent_notification(
    record: &PersistedAgentRuntimeRecord,
    reason: AgentAttentionReason,
    timestamp: &str,
) -> Option<Notification> {
    let superseded = record
        .attention_timestamp
        .as_deref()
        .is_some_and(|current| current != timestamp);
    let scheduled = record.labels.contains_key(SCHEDULE_RUN_LABEL);
    if record.internal || record.archived_at.is_some() || superseded || scheduled {
        return None;
    }
    let workspace_id = record.workspace_id.clone()?;
    let at = DateTime::parse_from_rfc3339(timestamp)
        .ok()?
        .with_timezone(&Utc);
    let ended = reason != AgentAttentionReason::Permission;
    let (kind, outcome) = match reason {
        AgentAttentionReason::Finished => (NotificationKind::AgentFinished, "finished"),
        AgentAttentionReason::Error => (NotificationKind::AgentFailed, "failed"),
        AgentAttentionReason::Permission => (
            NotificationKind::AgentPermission,
            "is waiting for permission",
        ),
    };
    let title = match record.title.as_deref().map(str::trim) {
        Some(title) if !title.is_empty() => format!("Agent \"{title}\" {outcome}"),
        _ => format!("Agent {outcome}"),
    };
    Some(Notification {
        id: format!("agent:{}:{timestamp}", record.id),
        workspace_id,
        scope: record.labels.get(BOT_SCOPE_LABEL).cloned(),
        kind,
        agent_id: Some(record.id.clone()),
        schedule_id: None,
        run_id: None,
        terminal_id: None,
        title,
        summary: (reason == AgentAttentionReason::Error)
            .then(|| record.last_error.clone())
            .flatten(),
        at,
        conversation_id: record
            .labels
            .get(BOT_CONVERSATION_LABEL)
            .and_then(|id| id.parse().ok()),
        ticket_id: record
            .labels
            .get(BOT_TICKET_LABEL)
            .and_then(|id| id.parse().ok()),
        ticket_title: None,
        turn_started_at: record
            .last_user_message_at
            .as_deref()
            .filter(|_| ended)
            .and_then(|started| DateTime::parse_from_rfc3339(started).ok())
            .map(|started| started.with_timezone(&Utc)),
    })
}

/// The notification for a terminal attention `payload` received at `now`; terminals have no
/// Bot origin, so it only wakes a Bot whose wake scope is `all`.
fn terminal_notification(payload: &Value, now: DateTime<Utc>) -> Option<Notification> {
    let terminal = payload["terminalId"].as_str()?;
    Some(Notification {
        id: format!("terminal:{terminal}:{}", now.timestamp_millis()),
        workspace_id: payload["workspaceId"].as_str()?.to_owned(),
        scope: None,
        kind: NotificationKind::TerminalAttention,
        agent_id: None,
        schedule_id: None,
        run_id: None,
        terminal_id: Some(terminal.to_owned()),
        title: payload["title"]
            .as_str()
            .unwrap_or("Terminal needs attention")
            .to_owned(),
        summary: payload["body"].as_str().map(str::to_owned),
        at: now,
        conversation_id: None,
        ticket_id: None,
        ticket_title: None,
        turn_started_at: None,
    })
}

/// The notification for settled occurrence `run` of `schedule`; `None` while it runs, when it
/// has no Workspace, and for a successful run of a heartbeat (an existing-Agent target), whose
/// Agent reports its own finished turn.
pub(super) fn schedule_notification(schedule: &Schedule, run: &Run) -> Option<Notification> {
    let (kind, outcome, text) = match run.status {
        RunStatus::Running => return None,
        RunStatus::Succeeded if matches!(schedule.target, Target::Agent { .. }) => return None,
        RunStatus::Succeeded => (
            NotificationKind::ScheduleSucceeded,
            "succeeded",
            &run.output,
        ),
        RunStatus::Failed => (NotificationKind::ScheduleFailed, "failed", &run.error),
    };
    let workspace_id = run
        .workspace_id
        .clone()
        .or_else(|| schedule.workspace_id.clone())?;
    let title = match schedule.name.as_deref() {
        Some(name) => format!("Schedule \"{name}\" run {outcome}"),
        None => format!("Schedule run {outcome}"),
    };
    Some(Notification {
        id: format!("schedule:{}:{}", schedule.id, run.id),
        workspace_id,
        scope: schedule.bot_scope.clone(),
        kind,
        agent_id: run.agent_id.clone(),
        schedule_id: Some(schedule.id.clone()),
        run_id: Some(run.id.clone()),
        terminal_id: None,
        title,
        summary: text
            .as_deref()
            .map(|text| text[..text.floor_char_boundary(SUMMARY_BYTES)].to_owned()),
        at: run.ended_at.unwrap_or(run.started_at),
        conversation_id: schedule.bot_conversation,
        ticket_id: None,
        ticket_title: None,
        turn_started_at: None,
    })
}

#[cfg(test)]
mod tests;
