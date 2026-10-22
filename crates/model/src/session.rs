//! Bounded connection observers and activity policy, without a transport/runtime dependency.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex, Weak};

use chrono::{DateTime, Utc};
use domain::session::SessionError;
use domain::session::protocol::{EventsRequest, Heartbeat, SessionEventKind};
use serde_json::Value;
use tokio::sync::broadcast;
use uuid::Uuid;

const PRESENCE_MS: i64 = 180_000;
const MAX_PENDING_EVENTS: usize = 64;
const MAX_PENDING_BYTES: usize = 1024 * 1024;

/// Transport-owned bounded delivery callback. Payload includes its subscription identity.
type EventSink = Arc<dyn Fn(SessionEventKind, Value) -> Result<(), SessionError> + Send + Sync>;

#[derive(Default)]
struct Presence {
    activity_ms: Option<i64>,
    heartbeat_ms: Option<i64>,
    visible: bool,
    focused_agent: Option<String>,
    focused_terminal: Option<String>,
    push_token: Option<String>,
}

impl fmt::Debug for Presence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Presence")
            .field("visible", &self.visible)
            .finish_non_exhaustive()
    }
}

#[derive(Default)]
struct State {
    connections: BTreeMap<String, Weak<Mutex<Presence>>>,
    listeners: Vec<Weak<Listener>>,
    attention: Option<broadcast::Sender<(SessionEventKind, Value)>>,
    publisher: Option<Arc<dyn crate::hub::EventPublisher>>,
}

/// Shared in-memory events and presence. Durable business state stays with its owning capability.
#[derive(Clone, Default)]
pub struct SessionEvents(Arc<Mutex<State>>);

impl fmt::Debug for SessionEvents {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionEvents { .. }")
    }
}

/// Presence lifetime owned by one authenticated connection.
#[derive(Debug)]
pub struct SessionConnection {
    id: String,
    presence: Arc<Mutex<Presence>>,
    events: SessionEvents,
}

struct Listener {
    id: String,
    connection: String,
    presence: Weak<Mutex<Presence>>,
    kinds: BTreeSet<SessionEventKind>,
    notifications: bool,
    workspace_id: Option<String>,
    sink: EventSink,
    delivery: Mutex<Delivery>,
}

#[derive(Default)]
struct Delivery {
    active: bool,
    closed: bool,
    bytes: usize,
    pending: VecDeque<(SessionEventKind, Value)>,
}

/// An observer starts paused; activate only after its RPC response enters the outbound queue.
pub struct SessionSubscription(Arc<Listener>);

impl fmt::Debug for SessionSubscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SessionSubscription")
            .field(&self.0.id)
            .finish()
    }
}

impl SessionEvents {
    /// Install the Host's independent post-commit publisher before accepting client connections.
    /// Its native notification decision is made before any connection focus filtering.
    pub fn set_publisher(&self, publisher: Arc<dyn crate::hub::EventPublisher>) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .publisher = Some(publisher);
    }

    /// Remove an invalid provider token from live connection owners before further heartbeats.
    pub fn clear_push_token(&self, token: &str) {
        let state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for presence in state.connections.values().filter_map(Weak::upgrade) {
            let mut presence = presence
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if presence.push_token.as_deref() == Some(token) {
                presence.push_token = None;
            }
        }
    }

    /// Observe unfocused attention independently of client connections, including after disconnect.
    /// The bounded channel retains at most 64 events; lagging consumers must report lost events.
    #[must_use]
    pub fn subscribe_attention(&self) -> broadcast::Receiver<(SessionEventKind, Value)> {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .attention
            .get_or_insert_with(|| broadcast::channel(64).0)
            .subscribe()
    }

    /// Return tokens whose fresh visible connections handle notifications locally.
    /// Tokens are opaque secrets and must never be logged.
    #[must_use]
    pub fn foreground_push_tokens(&self) -> BTreeSet<String> {
        let now = Utc::now().timestamp_millis();
        let state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .connections
            .values()
            .filter_map(Weak::upgrade)
            .filter_map(|presence| {
                let presence = presence
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if presence.visible
                    && presence
                        .heartbeat_ms
                        .is_some_and(|heartbeat| now.saturating_sub(heartbeat) <= PRESENCE_MS)
                {
                    presence.push_token.clone()
                } else {
                    None
                }
            })
            .collect()
    }

    /// Register presence for a new connection. Dropping it releases focus suppression.
    #[must_use]
    pub fn connect(&self) -> SessionConnection {
        let id = Uuid::new_v4().to_string();
        let presence = Arc::new(Mutex::new(Presence::default()));
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .connections
            .retain(|_, value| value.strong_count() != 0);
        state
            .connections
            .insert(id.clone(), Arc::downgrade(&presence));
        SessionConnection {
            id,
            presence,
            events: self.clone(),
        }
    }

    /// Publish after the owning service commits a change. Slow/closed observers cannot fail it.
    /// For attention, `payload.agentId` selects the focus target; notification is elected once
    /// across present connections and duplicate subscriptions. Every observer still gets state.
    pub fn publish(&self, kind: SessionEventKind, payload: &Value) {
        self.publish_at(kind, payload, Utc::now().timestamp_millis());
    }

    fn publish_to_center(&self, kind: SessionEventKind, payload: &Value) -> bool {
        let publisher = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .publisher
            .clone();
        publisher.is_some_and(|publisher| publisher.publish(kind.method(), payload))
    }

    fn publish_at(&self, kind: SessionEventKind, payload: &Value, now: i64) {
        if !payload.is_object() {
            return;
        }
        let attention = matches!(
            kind,
            SessionEventKind::AgentAttention | SessionEventKind::TerminalAttention
        );
        let center_notification = self.publish_to_center(kind, payload);
        let (listeners, presence, push) = {
            let mut state = self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state
                .listeners
                .retain(|listener| listener.strong_count() != 0);
            state
                .connections
                .retain(|_, presence| presence.strong_count() != 0);
            let listeners: Vec<_> = state
                .listeners
                .iter()
                .filter_map(Weak::upgrade)
                .filter(|listener| {
                    listener.kinds.contains(&kind)
                        && listener.presence.strong_count() != 0
                        && listener
                            .workspace_id
                            .as_deref()
                            .is_none_or(|id| payload["workspaceId"].as_str() == Some(id))
                })
                .collect();
            let presence: Vec<_> = state
                .connections
                .iter()
                .filter(|_| attention)
                .filter_map(|(id, value)| value.upgrade().map(|value| (id.clone(), value)))
                .collect();
            (listeners, presence, state.attention.clone())
        };
        let mut recipient = None;
        let mut latest = i64::MIN;
        let mut focused = false;
        if attention {
            let candidates: BTreeSet<_> = listeners
                .iter()
                .filter(|listener| listener.notifications)
                .map(|listener| listener.connection.as_str())
                .collect();
            for (id, presence) in presence {
                let presence = presence
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let Some(activity) = presence.activity_ms else {
                    continue;
                };
                if presence
                    .heartbeat_ms
                    .is_none_or(|heartbeat| now.saturating_sub(heartbeat) > PRESENCE_MS)
                {
                    continue;
                }
                let (focus, target) = if kind == SessionEventKind::TerminalAttention {
                    (&presence.focused_terminal, payload["terminalId"].as_str())
                } else {
                    (&presence.focused_agent, payload["agentId"].as_str())
                };
                focused |= presence.visible && focus.is_some() && focus.as_deref() == target;
                if activity > latest && candidates.contains(id.as_str()) {
                    latest = activity;
                    recipient = Some(id);
                }
            }
        }
        if attention
            && !focused
            && !center_notification
            && let Some(push) = push
        {
            let _ = push.send((kind, payload.clone()));
        }
        let mut notified = false;
        for listener in listeners {
            let mut value = payload.clone();
            value["subscriptionId"] = Value::String(listener.id.clone());
            if attention {
                let should_notify = !focused
                    && !notified
                    && listener.notifications
                    && recipient.as_deref() == Some(listener.connection.as_str());
                value["shouldNotify"] = Value::Bool(should_notify);
                notified |= should_notify;
            }
            if listener.deliver(kind, value).is_err() {
                tracing::debug!("session event observer closed");
            }
        }
    }
}

impl SessionConnection {
    /// Whether this connection still owns `token`; invalidation prevents heartbeat resurrection.
    #[must_use]
    pub fn owns_push_token(&self, token: &str) -> bool {
        self.presence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_token
            .as_deref()
            == Some(token)
    }

    /// Associate a durably registered token with this physical connection for local/push deduplication.
    pub fn set_push_token(&self, token: Option<String>) {
        self.presence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_token = token;
    }

    /// Return this connection's opaque identity.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Validate and replace activity. Future timestamps are clamped at receipt time.
    ///
    /// # Errors
    /// Rejects invalid timestamps or oversized/control-containing focus identities.
    pub fn heartbeat(&self, heartbeat: Heartbeat) -> Result<(), SessionError> {
        let activity = parse_time(&heartbeat.last_activity_at)?;
        if let Some(timestamp) = &heartbeat.app_visibility_changed_at {
            parse_time(timestamp)?;
        }
        for id in [&heartbeat.focused_agent_id, &heartbeat.focused_terminal_id]
            .into_iter()
            .flatten()
        {
            if id.len() > 128 || id.chars().any(char::is_control) {
                return Err(SessionError::Invalid);
            }
        }
        let mut presence = self.presence.lock().map_err(|_| SessionError::Closed)?;
        presence.activity_ms = Some(activity.min(Utc::now().timestamp_millis()));
        presence.heartbeat_ms = Some(Utc::now().timestamp_millis());
        presence.visible = heartbeat.app_visible;
        presence.focused_agent = heartbeat.focused_agent_id.filter(|id| !id.is_empty());
        presence.focused_terminal = heartbeat.focused_terminal_id.filter(|id| !id.is_empty());
        Ok(())
    }

    /// Prepare an independently owned subscription. The API applies its shared connection budget.
    ///
    /// # Errors
    /// Rejects oversized requests, unknown categories, or unavailable event producers.
    pub fn subscribe(
        &self,
        request: EventsRequest,
        sink: EventSink,
    ) -> Result<SessionSubscription, SessionError> {
        self.subscribe_filtered(request, None, sink)
    }

    /// Observe setup detail for exactly one Workspace on this physical connection.
    /// # Errors
    /// Rejects empty or oversized identities and unavailable observer state.
    pub fn subscribe_workspace_setup(
        &self,
        workspace_id: String,
        sink: EventSink,
    ) -> Result<SessionSubscription, SessionError> {
        if workspace_id.is_empty()
            || workspace_id.len() > 256
            || workspace_id.chars().any(char::is_control)
        {
            return Err(SessionError::Invalid);
        }
        self.subscribe_filtered(
            EventsRequest {
                events: vec!["workspace_setup_progress".to_owned()],
                notifications: false,
            },
            Some(workspace_id),
            sink,
        )
    }

    fn subscribe_filtered(
        &self,
        request: EventsRequest,
        workspace_id: Option<String>,
        sink: EventSink,
    ) -> Result<SessionSubscription, SessionError> {
        if request.events.len() > 32 {
            return Err(SessionError::Invalid);
        }
        let kinds = request
            .events
            .into_iter()
            .map(|event| {
                serde_json::from_value(Value::String(event)).map_err(|_| SessionError::Unsupported)
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        let listener = Arc::new(Listener {
            id: Uuid::new_v4().to_string(),
            connection: self.id.clone(),
            presence: Arc::downgrade(&self.presence),
            kinds,
            notifications: request.notifications,
            workspace_id,
            sink,
            delivery: Mutex::new(Delivery::default()),
        });
        let mut state = self.events.0.lock().map_err(|_| SessionError::Closed)?;
        state
            .listeners
            .retain(|listener| listener.strong_count() != 0);
        state.listeners.push(Arc::downgrade(&listener));
        Ok(SessionSubscription(listener))
    }
}

impl Listener {
    fn deliver(&self, kind: SessionEventKind, payload: Value) -> Result<(), SessionError> {
        let mut delivery = self.delivery.lock().map_err(|_| SessionError::Closed)?;
        if delivery.closed {
            return Err(SessionError::Closed);
        }
        if delivery.active {
            let result = (self.sink)(kind, payload);
            delivery.closed = result.is_err();
            return result;
        }
        let bytes = serde_json::to_vec(&payload)
            .map_err(|_| SessionError::Invalid)?
            .len();
        if delivery.pending.len() >= MAX_PENDING_EVENTS
            || delivery.bytes.saturating_add(bytes) > MAX_PENDING_BYTES
        {
            delivery.closed = true;
            delivery.pending.clear();
            return Err(SessionError::Closed);
        }
        delivery.bytes += bytes;
        delivery.pending.push_back((kind, payload));
        Ok(())
    }
}

impl SessionSubscription {
    /// Return the connection-owned release identity.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.0.id
    }

    /// Flush bounded pending events in order after the response has been enqueued.
    ///
    /// # Errors
    /// Returns `Closed` when the observer overflowed or the transport rejects delivery.
    pub fn activate(&self) -> Result<(), SessionError> {
        let mut delivery = self.0.delivery.lock().map_err(|_| SessionError::Closed)?;
        if delivery.closed {
            return Err(SessionError::Closed);
        }
        while let Some((kind, payload)) = delivery.pending.pop_front() {
            if (self.0.sink)(kind, payload).is_err() {
                delivery.closed = true;
                delivery.pending.clear();
                return Err(SessionError::Closed);
            }
        }
        delivery.bytes = 0;
        delivery.active = true;
        Ok(())
    }
}

impl Drop for SessionSubscription {
    fn drop(&mut self) {
        let mut delivery = self
            .0
            .delivery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        delivery.closed = true;
        delivery.pending.clear();
    }
}

fn parse_time(value: &str) -> Result<i64, SessionError> {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.timestamp_millis())
        .map_err(|_| SessionError::Invalid)
}

#[cfg(test)]
mod tests;
