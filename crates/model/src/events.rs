//! Bounded, paused observers shared by capability-owned event producers.

use std::collections::{BTreeSet, VecDeque};
use std::sync::{Arc, Mutex, Weak};

use serde_json::Value;

use crate::ServerMessage;
use crate::outbound::{Outbound, QueueError};

/// An ephemeral fan-out hub. Business owners serialize snapshot reads with registration.
#[derive(Debug, Clone, Default)]
pub struct EventHub(Arc<Mutex<State>>);

#[derive(Debug, Default)]
struct State {
    listeners: Vec<Weak<Listener>>,
    publisher: Option<Arc<dyn crate::hub::EventPublisher>>,
}

#[derive(Debug)]
struct Listener {
    id: String,
    include_id: bool,
    keys: BTreeSet<String>,
    outbound: Outbound,
    delivery: Mutex<Delivery>,
}

#[derive(Debug, Default)]
struct Delivery {
    active: bool,
    closed: bool,
    bytes: usize,
    pending: VecDeque<ServerMessage>,
}

/// A connection-owned observer; dropping it atomically stops delivery.
#[derive(Debug)]
pub struct Subscription(Arc<Listener>);

impl EventHub {
    /// Install a Host publisher for committed events, independent of per-resource subscriptions.
    pub fn set_publisher(&self, publisher: Arc<dyn crate::hub::EventPublisher>) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .publisher = Some(publisher);
    }

    /// Register a paused observer for exact business keys with a host-generated release ID.
    #[must_use]
    pub fn subscribe(
        &self,
        id: String,
        keys: BTreeSet<String>,
        outbound: Outbound,
    ) -> Subscription {
        self.register(id, keys, outbound, true)
    }

    /// Register a paused operation observer whose events omit the connection subscription ID.
    /// `id` remains local to the returned guard; dropping it ends the operation's observation.
    #[must_use]
    pub fn observe(&self, id: String, keys: BTreeSet<String>, outbound: Outbound) -> Subscription {
        self.register(id, keys, outbound, false)
    }

    fn register(
        &self,
        id: String,
        keys: BTreeSet<String>,
        outbound: Outbound,
        include_id: bool,
    ) -> Subscription {
        let listener = Arc::new(Listener {
            id,
            include_id,
            keys,
            outbound,
            delivery: Mutex::new(Delivery::default()),
        });
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .listeners
            .retain(|listener| listener.strong_count() != 0);
        state.listeners.push(Arc::downgrade(&listener));
        Subscription(listener)
    }

    /// Publish committed state. Overflow closes the observer's transport rather than losing data.
    pub fn publish(&self, key: &str, method: &str, params: &Value) {
        if !params.is_object() {
            return;
        }
        let (listeners, publisher): (Vec<_>, _) = {
            let mut state = self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state
                .listeners
                .retain(|listener| listener.strong_count() != 0);
            let listeners = state
                .listeners
                .iter()
                .filter_map(Weak::upgrade)
                .filter(|listener| listener.keys.contains(key))
                .collect();
            (listeners, state.publisher.clone())
        };
        if let Some(publisher) = publisher {
            publisher.publish(method, params);
        }
        for listener in listeners {
            let mut params = params.clone();
            if listener.include_id {
                params["subscriptionId"] = Value::String(listener.id.clone());
            }
            let message = ServerMessage::Event {
                method: method.to_owned(),
                params,
            };
            let mut delivery = listener
                .delivery
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if delivery.closed {
                continue;
            }
            if delivery.active {
                if listener.outbound.send(&message).is_err() {
                    delivery.closed = true;
                    listener.outbound.failure().cancel();
                }
            } else {
                let bytes = serde_json::to_vec(&message).map_or(usize::MAX, |bytes| bytes.len());
                if delivery.pending.len() >= 64
                    || delivery.bytes.saturating_add(bytes) > 1024 * 1024
                {
                    delivery.closed = true;
                    delivery.pending.clear();
                    listener.outbound.failure().cancel();
                } else {
                    delivery.bytes += bytes;
                    delivery.pending.push_back(message);
                }
            }
        }
    }
}

impl Subscription {
    /// Return the release identity belonging to this observer.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.0.id
    }

    /// Flush queued events after the response has entered the same outbound queue.
    /// # Errors
    /// Returns a queue error if pending delivery overflowed or the connection is closed.
    pub fn activate(&self) -> Result<(), QueueError> {
        let mut delivery = self
            .0
            .delivery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if delivery.closed {
            return Err(QueueError::Full);
        }
        while let Some(message) = delivery.pending.pop_front() {
            if let Err(error) = self.0.outbound.send(&message) {
                delivery.closed = true;
                self.0.outbound.failure().cancel();
                return Err(error);
            }
        }
        delivery.bytes = 0;
        delivery.active = true;
        Ok(())
    }
}

impl Drop for Subscription {
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

#[cfg(test)]
mod tests;
