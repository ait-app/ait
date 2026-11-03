//! Latest Timeline markers per Agent, kept in memory instead of the durable journal.
//!
//! A marker only tells clients to re-read the Host's committed history. A newer update therefore
//! supersedes an older one, and a reset supersedes every earlier marker of its Agent; at most one
//! reset and one later update are pending per Agent. Markers are lost on restart, which clients
//! already recover from by reading the Host's history when they reconnect.

use std::collections::BTreeMap;
use std::ops::Bound;

use model::hub::Scope;
use serde_json::Value;

/// One Timeline marker publication for `agent`, as sent to `hub.events.publish`.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Marker {
    pub(super) agent: String,
    pub(super) publication: Value,
}

impl Marker {
    fn reset(&self) -> bool {
        self.publication["params"]["reset"] == true
    }
}

#[derive(Debug, Default)]
struct Pending {
    reset: Option<Value>,
    update: Option<Value>,
}

/// Pending markers for one publication scope, delivered round-robin across Agents.
#[derive(Debug, Default)]
pub(super) struct Markers {
    scope: Option<Scope>,
    agents: BTreeMap<String, Pending>,
    cursor: Option<String>,
}

impl Markers {
    /// Record `marker` under `scope`, discarding markers held for any previous scope.
    pub(super) fn insert(&mut self, scope: &Scope, marker: Marker) {
        if self.scope.as_ref() != Some(scope) {
            self.agents.clear();
            self.cursor = None;
            self.scope = Some(scope.clone());
        }
        let reset = marker.reset();
        let pending = self.agents.entry(marker.agent).or_default();
        if reset {
            pending.update = None;
            pending.reset = Some(marker.publication);
        } else {
            pending.update = Some(marker.publication);
        }
    }

    /// Remove the next marker to deliver for `scope`: an Agent's reset precedes its update,
    /// and Agents take turns so one busy Agent cannot starve the others.
    pub(super) fn take(&mut self, scope: &Scope) -> Option<Marker> {
        if self.scope.as_ref() != Some(scope) {
            return None;
        }
        let after = self
            .cursor
            .as_ref()
            .map_or(Bound::Unbounded, Bound::Excluded);
        let agent = self
            .agents
            .range::<String, _>((after, Bound::Unbounded))
            .next()
            .or_else(|| self.agents.iter().next())
            .map(|(agent, _)| agent.clone())?;
        let pending = self.agents.get_mut(&agent)?;
        let publication = pending.reset.take().or_else(|| pending.update.take())?;
        if pending.reset.is_none() && pending.update.is_none() {
            self.agents.remove(&agent);
        }
        self.cursor = Some(agent.clone());
        Some(Marker { agent, publication })
    }

    /// Return an undelivered `marker` unless a newer marker of its kind superseded it meanwhile.
    pub(super) fn restore(&mut self, scope: &Scope, marker: Marker) {
        if self.scope.as_ref() != Some(scope) {
            return;
        }
        let reset = marker.reset();
        let pending = self.agents.entry(marker.agent).or_default();
        if reset {
            if pending.reset.is_none() {
                pending.reset = Some(marker.publication);
            }
        } else if pending.reset.is_none() && pending.update.is_none() {
            pending.update = Some(marker.publication);
        }
    }
}

#[cfg(test)]
mod tests;
