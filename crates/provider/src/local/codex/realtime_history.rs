//! Page-local native projection with bounded display history and a narrow legacy fallback.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};

use serde_json::{Value, json};

use super::{Transport, discovery, native_sessions, realtime_timeline};
use crate::ports::agent_session::AgentSessionError;
use crate::ports::native_history::{HistoryError, HistoryFailure};
use crate::protocol::timeline::NativeItem;

const METHOD: &str = "thread/timeline/list";
const MAX_ENTRIES: usize = 32_768;
const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_PAGES: usize = 512;

type ItemKey = (String, String);

#[derive(Debug)]
enum Entry {
    TurnStarted(String),
    Ordinary(ItemKey),
    Promotion(ItemKey),
    Voice(NativeItem),
}

#[derive(Debug, Default)]
struct Projection {
    entries: Vec<Entry>,
    ordinary: BTreeMap<ItemKey, Vec<NativeItem>>,
    stamps: BTreeMap<String, String>,
    voice: realtime_timeline::Timeline,
    voice_ids: BTreeSet<String>,
    count: usize,
    bytes: usize,
    position: Option<u64>,
}

impl Projection {
    fn error(&self, page: usize, failure: HistoryFailure) -> AgentSessionError {
        AgentSessionError::History(HistoryError {
            method: METHOD,
            page,
            bytes: self.bytes,
            failure,
        })
    }

    fn push(
        &mut self,
        entry: &Value,
        page: usize,
        created: &str,
        images: &crate::local::images::ImageStore,
    ) -> Result<(), AgentSessionError> {
        self.count += 1;
        if self.count > MAX_ENTRIES {
            return Err(self.error(
                page,
                HistoryFailure::Limit {
                    resource: "canonical entries",
                    limit: MAX_ENTRIES,
                    actual: self.count,
                },
            ));
        }
        let position = entry["position"].as_u64().ok_or_else(|| {
            self.error(
                page,
                HistoryFailure::Protocol("missing or invalid position"),
            )
        })?;
        if self.position.is_some_and(|previous| position >= previous) {
            return Err(self.error(
                page,
                HistoryFailure::Protocol("duplicate or out-of-order position"),
            ));
        }
        self.position = Some(position);
        self.project(entry, created, images)
            .map_err(|error| match error {
                AgentSessionError::History(_) => error,
                _ => self.error(
                    page,
                    HistoryFailure::Protocol("invalid entry or image projection"),
                ),
            })?;
        if self.bytes > MAX_BYTES {
            return Err(self.error(
                page,
                HistoryFailure::Limit {
                    resource: "display bytes",
                    limit: MAX_BYTES,
                    actual: self.bytes,
                },
            ));
        }
        Ok(())
    }

    fn project(
        &mut self,
        entry: &Value,
        created: &str,
        images: &crate::local::images::ImageStore,
    ) -> Result<(), AgentSessionError> {
        let projected = match entry["type"].as_str() {
            Some("turnStarted") => {
                let turn = native_sessions::text(entry, "turnId")?.to_owned();
                if !entry["startedAt"].is_null() {
                    self.stamps.insert(
                        turn.clone(),
                        native_sessions::timestamp(&entry["startedAt"])?,
                    );
                }
                Entry::TurnStarted(turn)
            }
            Some("item") => {
                let turn = native_sessions::text(entry, "turnId")?;
                let id = native_sessions::text(&entry["item"], "id")?;
                let key = (turn.to_owned(), id.to_owned());
                if self.ordinary.contains_key(&key) {
                    return Err(AgentSessionError::Failed);
                }
                let items = discovery::timeline_items(&entry["item"], turn, created, images)?;
                for item in &items {
                    self.bytes += display_bytes(item)?;
                }
                self.ordinary.insert(key.clone(), items);
                Entry::Ordinary(key)
            }
            Some("realtime") if entry["item"]["type"] == "bemItemPromoted" => {
                let native = &entry["item"];
                Entry::Promotion((
                    native_sessions::text(native, "turnId")?.to_owned(),
                    native_sessions::text(native, "itemId")?.to_owned(),
                ))
            }
            Some("realtime") => {
                let Some(item) = realtime_timeline::project(&entry["item"], created)? else {
                    return Ok(());
                };
                if !self.voice_ids.insert(item.key.clone()) {
                    return Err(AgentSessionError::Failed);
                }
                self.voice.remember(&item)?;
                self.bytes += display_bytes(&item)?;
                Entry::Voice(item)
            }
            Some("turnCompleted") => return Ok(()),
            _ => return Err(AgentSessionError::Failed),
        };
        self.entries.push(projected);
        Ok(())
    }

    fn finish(mut self, created: &str, page: usize) -> Result<Vec<NativeItem>, AgentSessionError> {
        // Validate associations before removing items: a source can precede or follow a promotion.
        if self
            .entries
            .iter()
            .any(|entry| matches!(entry, Entry::Promotion(key) if !self.ordinary.contains_key(key)))
        {
            return Err(self.error(
                page,
                HistoryFailure::Protocol("promotion has no source item"),
            ));
        }
        let mut timestamp = created.to_owned();
        let mut output = Vec::new();
        let mut identities = BTreeSet::new();
        // Pages arrived newest first. Only compact projections survive to this chronological pass.
        for entry in self.entries.into_iter().rev() {
            let items = match entry {
                Entry::TurnStarted(turn) => {
                    if let Some(stamp) = self.stamps.get(&turn) {
                        timestamp.clone_from(stamp);
                    }
                    continue;
                }
                Entry::Ordinary(key) | Entry::Promotion(key) => {
                    self.ordinary.remove(&key).unwrap_or_default()
                }
                Entry::Voice(item) => vec![item],
            };
            for mut item in items {
                item.timestamp.clone_from(
                    item.turn_id
                        .as_ref()
                        .and_then(|turn| self.stamps.get(turn))
                        .unwrap_or(&timestamp),
                );
                if self.voice.retain_handoff(&mut item) && identities.insert(item.key.clone()) {
                    output.push(item);
                }
            }
        }
        Ok(output)
    }
}

#[derive(Default)]
struct ByteCount(usize);

impl Write for ByteCount {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn display_bytes(item: &NativeItem) -> Result<usize, AgentSessionError> {
    let mut count = ByteCount::default();
    serde_json::to_writer(&mut count, item).map_err(|_| AgentSessionError::Failed)?;
    Ok(count.0)
}

/// Read and project one page at a time, returning `None` only for an unsupported first RPC.
/// Returns payload-free RPC, protocol or display-budget errors; native history is never changed.
pub(super) async fn read(
    transport: &mut Transport,
    thread: &Value,
    images: &crate::local::images::ImageStore,
) -> Result<Option<Vec<NativeItem>>, AgentSessionError> {
    let id = native_sessions::text(thread, "id")?;
    let created = native_sessions::timestamp(&thread["createdAt"])?;
    let mut cursor = None::<String>;
    let mut cursors = BTreeSet::new();
    let mut projection = Projection::default();
    for page_number in 1..=MAX_PAGES {
        let result = transport
            .request(METHOD, json!({"threadId":id,"cursor":cursor,"limit":100}))
            .await;
        let mut page = match result {
            Err(AgentSessionError::Unavailable) if page_number == 1 => return Ok(None),
            Ok(page) => page,
            Err(error) => {
                let reason = match error {
                    AgentSessionError::Unavailable => "method unavailable after pagination began",
                    AgentSessionError::Failed => "transport or provider failure",
                    AgentSessionError::Rejected => "request rejected",
                    AgentSessionError::Busy => "session busy",
                    AgentSessionError::History(_) => return Err(error),
                };
                return Err(projection.error(page_number, HistoryFailure::Rpc(reason)));
            }
        };
        let Some(Value::Array(data)) = page.as_object_mut().and_then(|page| page.remove("data"))
        else {
            return Err(projection.error(
                page_number,
                HistoryFailure::Protocol("data is not an array"),
            ));
        };
        // Release each raw row after projection, before requesting the next page.
        for entry in data.into_iter().rev() {
            projection.push(&entry, page_number, &created, images)?;
        }
        cursor = match page
            .as_object_mut()
            .and_then(|page| page.remove("nextCursor"))
        {
            Some(Value::Null) => return projection.finish(&created, page_number).map(Some),
            Some(Value::String(cursor))
                if !cursor.is_empty()
                    && cursor.len() <= 4096
                    && !cursor.chars().any(char::is_control) =>
            {
                Some(cursor)
            }
            _ => {
                return Err(
                    projection.error(page_number, HistoryFailure::Protocol("invalid next cursor"))
                );
            }
        };
        if !cursors.insert(cursor.clone()) {
            return Err(projection.error(page_number, HistoryFailure::Protocol("cursor cycle")));
        }
    }
    Err(projection.error(
        MAX_PAGES,
        HistoryFailure::Limit {
            resource: "pages",
            limit: MAX_PAGES,
            actual: MAX_PAGES + 1,
        },
    ))
}

#[cfg(test)]
mod tests;
