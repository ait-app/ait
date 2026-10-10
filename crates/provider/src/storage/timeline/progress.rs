use std::collections::{BTreeMap, BTreeSet};

use model::ErrorCode;
use rusqlite::{Connection, OptionalExtension, params};

use super::{MAX_ENTRY_BYTES, NativeItem, Row, Timeline, epoch, io};

impl Timeline {
    /// Persist and publish one retry-stable incremental item in the same cursor space as history.
    /// Text items are additive deltas; tool items are bounded running snapshots.
    /// # Errors
    /// Returns changed replay, already completed item, oversized data or storage failures.
    pub(crate) fn progress(
        &self,
        agent: &str,
        provider: &str,
        observation: &str,
        entry: &NativeItem,
    ) -> Result<(), ErrorCode> {
        if !matches!(
            entry.item["type"].as_str(),
            Some("assistant_message" | "reasoning" | "tool_call")
        ) || !entry.key.starts_with("native:")
            || observation.is_empty()
        {
            return Err(ErrorCode::InvalidMessage);
        }
        let bytes = serde_json::to_string(entry).map_err(io)?;
        if bytes.len() > MAX_ENTRY_BYTES {
            tracing::warn!(
                agent_id = agent,
                provider,
                item_type = entry.item["type"].as_str().unwrap_or("unknown"),
                size_bytes = bytes.len(),
                limit_bytes = MAX_ENTRY_BYTES,
                "Timeline progress item exceeds size limit"
            );
            return Err(ErrorCode::ResourceExhausted);
        }
        let mut database = self.database.lock().map_err(io)?;
        let transaction = database.transaction().map_err(io)?;
        let epoch = epoch(&transaction, agent)?;
        let identity = format!("progress:{observation}");
        let existing: Option<String> = transaction
            .query_row(
                "SELECT entry FROM progress WHERE agent=? AND identity=?",
                params![agent, identity],
                |row| row.get(0),
            )
            .optional()
            .map_err(io)?;
        if let Some(previous) = existing {
            return if previous == bytes {
                Ok(())
            } else {
                Err(ErrorCode::IdempotencyConflict)
            };
        }
        let completed: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM entries WHERE agent=? AND identity=?)",
                params![agent, entry.key],
                |row| row.get(0),
            )
            .map_err(io)?;
        if completed {
            return Err(ErrorCode::IdempotencyConflict);
        }
        let seq = next(&transaction, agent)?;
        transaction.execute(
            "INSERT INTO progress(agent,seq,identity,native_key,provider,entry) VALUES(?,?,?,?,?,?)",
            params![agent,seq,identity,entry.key,provider,bytes],
        ).map_err(io)?;
        transaction.commit().map_err(io)?;
        self.publish(agent, provider, &epoch, &[(seq, entry.clone())]);
        Ok(())
    }
}

pub(super) fn next(database: &Connection, agent: &str) -> Result<u64, ErrorCode> {
    let previous: u64 = database
        .query_row(
            "SELECT MAX(COALESCE((SELECT MAX(seq) FROM entries WHERE agent=?1),0),
            COALESCE((SELECT MAX(seq) FROM progress WHERE agent=?1),0))",
            [agent],
            |row| row.get(0),
        )
        .map_err(io)?;
    previous.checked_add(1).ok_or(ErrorCode::ResourceExhausted)
}

pub(super) fn completion(
    database: &Connection,
    agent: &str,
    entry: &NativeItem,
) -> Result<NativeItem, ErrorCode> {
    let mut projected = entry.clone();
    if !additive(entry) {
        return Ok(projected);
    }
    let mut query = database
        .prepare("SELECT entry FROM progress WHERE agent=? AND native_key=? ORDER BY seq")
        .map_err(io)?;
    let rows = query
        .query_map(params![agent, entry.key], |row| row.get::<_, String>(0))
        .map_err(io)?;
    let mut prefix = String::new();
    for row in rows {
        let partial: NativeItem = serde_json::from_str(&row.map_err(io)?).map_err(io)?;
        extend(&mut prefix, &partial)?;
    }
    suffix(&mut projected, &prefix)?;
    Ok(projected)
}

pub(super) fn read(database: &Connection, agent: &str) -> Result<Vec<Row>, ErrorCode> {
    let mut query = database
        .prepare(
            "SELECT seq,provider,entry,0 FROM entries WHERE agent=?1
         UNION ALL SELECT seq,provider,entry,1 FROM progress WHERE agent=?1 ORDER BY seq",
        )
        .map_err(io)?;
    let rows = query
        .query_map([agent], |row| {
            Ok((
                row.get::<_, u64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, bool>(3)?,
            ))
        })
        .map_err(io)?;
    let mut prefixes: BTreeMap<String, String> = BTreeMap::new();
    let mut result = Vec::new();
    for row in rows {
        let (seq, provider, bytes, partial) = row.map_err(io)?;
        let mut entry: NativeItem = serde_json::from_str(&bytes).map_err(io)?;
        if additive(&entry) {
            if partial {
                extend(prefixes.entry(entry.key.clone()).or_default(), &entry)?;
            } else if let Some(prefix) = prefixes.remove(&entry.key) {
                suffix(&mut entry, &prefix)?;
            }
        }
        result.push(Row {
            seq,
            provider,
            entry,
        });
    }
    Ok(result)
}

pub(super) fn covered(
    database: &Connection,
    agent: &str,
    entries: &[NativeItem],
) -> Result<bool, ErrorCode> {
    let native: BTreeMap<_, _> = entries
        .iter()
        .map(|entry| (entry.key.as_str(), entry))
        .collect();
    let mut first_users = BTreeMap::new();
    for entry in entries {
        if entry.item["type"] == "user_message"
            && let Some(turn) = entry.turn_id.as_deref()
        {
            first_users.entry(turn).or_insert(entry.key.as_str());
        }
    }
    let mut query = database
        .prepare("SELECT identity FROM (
            SELECT seq,identity FROM entries WHERE agent=?1 AND json_extract(entry,'$.item.type')!='plugin'
            UNION ALL SELECT seq,native_key FROM progress WHERE agent=?1) ORDER BY seq")
        .map_err(io)?;
    let rows = query
        .query_map([agent], |row| row.get::<_, String>(0))
        .map_err(io)?;
    let mut seen_users = BTreeSet::new();
    for row in rows {
        let key = row.map_err(io)?;
        let Some(entry) = native.get(key.as_str()) else {
            return Ok(false);
        };
        if entry.item["type"] == "user_message" {
            seen_users.insert(key);
        } else if let Some(user) = entry
            .turn_id
            .as_deref()
            .and_then(|turn| first_users.get(turn))
            && !seen_users.contains(*user)
        {
            // Keep the initial input ahead of its reply. A later steer in the same native turn
            // may legitimately follow output that was already published.
            return Ok(false);
        }
    }
    // A crash can leave only a provisional prefix. Refresh may replace it when native history
    // recovered different text; ordinary append must continue to reject that inconsistency.
    for entry in entries {
        match completion(database, agent, entry) {
            Ok(_) => {}
            Err(ErrorCode::IdempotencyConflict) => return Ok(false),
            Err(error) => return Err(error),
        }
    }
    Ok(true)
}

/// Preserve first observations when authoritative history replaces a display generation.
/// Submitted inputs match user rows by client identity; other items retain their keys and turns.
/// Storage errors are returned before the visible generation is changed.
pub(super) fn retain_timestamps(
    database: &Connection,
    agent: &str,
    previous: &[Row],
    entries: &[NativeItem],
) -> Result<Vec<NativeItem>, ErrorCode> {
    let mut timestamps = BTreeMap::new();
    let mut inputs = BTreeMap::new();
    for row in previous {
        timestamps
            .entry(row.entry.key.clone())
            .or_insert_with(|| (row.entry.turn_id.clone(), row.entry.timestamp.clone()));
        if row.entry.item["type"] == "user_message"
            && let Some(client) = row.entry.item["clientMessageId"].as_str()
        {
            inputs.entry(client).or_insert(row.entry.timestamp.as_str());
        }
    }
    let mut query = database.prepare("SELECT native_key,json_extract(entry,'$.turnId'),json_extract(entry,'$.timestamp') FROM progress WHERE agent=? ORDER BY seq").map_err(io)?;
    let rows = query
        .query_map([agent], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(io)?;
    for row in rows {
        let (key, turn, timestamp) = row.map_err(io)?;
        timestamps.entry(key).or_insert((turn, timestamp));
    }
    Ok(entries
        .iter()
        .map(|entry| {
            let mut retained = entry.clone();
            if let Some(timestamp) = timestamps
                .get(&entry.key)
                .filter(|(turn, _)| turn == &entry.turn_id)
                .map(|(_, timestamp)| timestamp.as_str())
                .or_else(|| {
                    if entry.item["type"] == "user_message" {
                        entry.item["clientMessageId"]
                            .as_str()
                            .and_then(|client| inputs.get(client).copied())
                    } else {
                        None
                    }
                })
            {
                timestamp.clone_into(&mut retained.timestamp);
            }
            retained
        })
        .collect())
}

fn additive(entry: &NativeItem) -> bool {
    matches!(
        entry.item["type"].as_str(),
        Some("assistant_message" | "reasoning")
    )
}

fn extend(prefix: &mut String, entry: &NativeItem) -> Result<(), ErrorCode> {
    let delta = entry.item["text"].as_str().ok_or(ErrorCode::AgentIo)?;
    if prefix.len().saturating_add(delta.len()) > MAX_ENTRY_BYTES {
        tracing::warn!(
            item_type = entry.item["type"].as_str().unwrap_or("unknown"),
            size_bytes = prefix.len().saturating_add(delta.len()),
            limit_bytes = MAX_ENTRY_BYTES,
            "Timeline accumulated text exceeds size limit"
        );
        return Err(ErrorCode::ResourceExhausted);
    }
    prefix.push_str(delta);
    Ok(())
}

fn suffix(entry: &mut NativeItem, prefix: &str) -> Result<(), ErrorCode> {
    let complete = entry.item["text"].as_str().ok_or(ErrorCode::AgentIo)?;
    let tail = complete
        .strip_prefix(prefix)
        .ok_or(ErrorCode::IdempotencyConflict)?;
    entry.item["text"] = serde_json::json!(tail);
    Ok(())
}

#[cfg(test)]
mod tests;
