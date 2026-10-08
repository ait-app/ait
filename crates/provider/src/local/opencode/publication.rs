//! Assign display order only after reading the native predecessors of each streamed text item.
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

use super::{
    failure, projection,
    session::Connection,
    types::{Fault, ProgressEvent, ProgressSink, ProtocolError},
};

/// Per-input publication state; completed predecessors suppress late text observations.
#[derive(Default)]
pub(super) struct Publication {
    streamed: HashSet<String>,
    finalized: HashSet<String>,
    deferred: bool,
}

#[cfg(test)]
mod tests;

impl Publication {
    /// Publish native predecessors before the first delta for each text item.
    /// Defers to final history if the prefix is unsettled; propagates invalid history and limits.
    pub(super) async fn report(
        &mut self,
        connection: &Connection,
        sink: &Arc<dyn ProgressSink>,
        event: ProgressEvent,
    ) -> Result<(), ProtocolError> {
        let ProgressEvent::TextDelta { id, .. } = &event else {
            return Err(failure(
                Fault::ProviderFailed,
                "unexpected streamed history",
            ));
        };
        let api = &connection.runtime.api;
        let native = id;
        if self.deferred || self.finalized.contains(&projection::key(native)) {
            return Ok(());
        }
        if !self.streamed.contains(id) {
            if self.streamed.len() as u64 >= connection.limits.max_steps {
                return Err(failure(
                    Fault::RunLimitExceeded,
                    "too many streamed text items",
                ));
            }
            let raw = api.history(&connection.prepared.id).await?;
            api.validate_budget(&raw, &connection.prepared, connection.limits)?;
            let Some(records) = api.before_text(
                &connection.prepared.id,
                &connection.prepared.input_id,
                id,
                &raw,
            )?
            else {
                self.deferred = true;
                return Ok(());
            };
            let entries = projection::records(&records, &BTreeMap::new()).map_err(|_| {
                failure(Fault::ProviderFailed, "invalid native stream predecessors")
            })?;
            if entries
                .first()
                .is_none_or(|entry| entry.item["type"] != "user_message")
            {
                return Err(failure(
                    Fault::RunRecoveryFailed,
                    "admitted input has no text",
                ));
            }
            for entry in entries {
                if self.finalized.insert(entry.key.clone()) {
                    sink.report(ProgressEvent::Timeline(Box::new(entry))).await;
                }
            }
            self.streamed.insert(id.clone());
        }
        sink.report(event).await;
        Ok(())
    }
}
