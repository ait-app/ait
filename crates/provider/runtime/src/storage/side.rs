//! Durable BTW side-question store (shared BTW contract).
//!
//! Owns topic identity, ask receipts, one-active enforcement and the frozen
//! display-context snapshot for every provider. The main input FIFO and the
//! main timeline are never touched: side questions are a separate projection.
//! Provider adapters execute behind [`provider_core::ports::side_questions`]; this
//! store never parses native payloads.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::storage::timeline::Timeline;
use provider_core::protocol::side_questions::{
    self, MAX_SNAPSHOT_BYTES, SideAskResponse, SideContextSnapshot, SideQuestionsState,
    SideReceiptState, SideStrategy, SideTopic,
};

mod projection;

/// SQLite application family for the side-question store.
const FAMILY: i32 = 0x4149_5439;
/// Schema version.
const VERSION: i32 = 1;

const SCHEMA: &str = "
CREATE TABLE side_topics (
    agent_id TEXT NOT NULL,
    topic_id TEXT NOT NULL,
    strategy TEXT NOT NULL,
    snapshot TEXT NOT NULL,
    record TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (agent_id, topic_id)
);
CREATE TABLE side_asks (
    agent_id TEXT NOT NULL,
    request_id TEXT NOT NULL,
    topic_id TEXT,
    question TEXT NOT NULL,
    record_id TEXT,
    receipt TEXT NOT NULL,
    strategy TEXT,
    snapshot TEXT,
    active INTEGER NOT NULL DEFAULT 1,
    reason TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (agent_id, request_id)
);
";

/// One durable ask receipt row.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AskRecord {
    /// Topic assigned once accepted.
    pub(crate) topic_id: Option<String>,
    /// Question text as submitted.
    pub(crate) question: String,
    /// Follow-up target as submitted.
    pub(crate) record_id: Option<String>,
    /// Durable acceptance state.
    pub(crate) receipt: SideReceiptState,
    /// Strategy frozen at acceptance.
    pub(crate) strategy: Option<SideStrategy>,
    /// Stable rejection reason.
    pub(crate) reason: Option<String>,
}

/// Outcome of reserving an ask idempotency key.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AskClaim {
    /// First observation; the caller may execute.
    New,
    /// The same request was already accepted; replay the stored receipt.
    Accepted(AskRecord),
    /// The same request was already rejected; replay the stored rejection.
    Rejected(AskRecord),
    /// Acceptance is uncertain; reconcile before any new execution.
    Uncertain(AskRecord),
    /// The key was used for different parameters.
    Conflict,
}

/// Durable side-question store shared by the manager and RPC projections.
#[derive(Debug, Clone)]
pub struct SideStore {
    database: Arc<Mutex<Connection>>,
    directory: Option<PathBuf>,
    live: Arc<Mutex<std::collections::BTreeMap<String, SideQuestionsState>>>,
}

impl SideStore {
    /// Open a dedicated store at `path`, creating parent directories.
    ///
    /// # Errors
    /// Returns Agent I/O errors for unreadable, incompatible or unwritable storage.
    pub fn open(path: &Path) -> Result<Self, crate::rpc::ErrorCode> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        }
        let mut connection = Connection::open(path).map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(1))
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        let application: i32 = connection
            .pragma_query_value(None, "application_id", |row| row.get(0))
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        let version: i32 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        if application == 0 && version == 0 {
            let transaction = connection
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
            transaction
                .execute_batch(SCHEMA)
                .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
            transaction
                .pragma_update(None, "application_id", FAMILY)
                .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
            transaction
                .pragma_update(None, "user_version", VERSION)
                .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
            transaction
                .commit()
                .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        } else if application != FAMILY || version != VERSION {
            return Err(crate::rpc::ErrorCode::AgentIo);
        }
        Ok(Self {
            database: Arc::new(Mutex::new(connection)),
            directory: Some(path.with_extension("aux")),
            live: Arc::default(),
        })
    }

    /// In-memory store for tests.
    ///
    /// # Errors
    /// Returns Agent I/O errors when the schema cannot be created.
    #[cfg(test)]
    pub(crate) fn memory() -> Result<Self, crate::rpc::ErrorCode> {
        let connection =
            Connection::open_in_memory().map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        connection
            .execute_batch(SCHEMA)
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        Ok(Self {
            database: Arc::new(Mutex::new(connection)),
            directory: None,
            live: Arc::default(),
        })
    }

    /// Delete a retired Agent's auxiliary receipts and history after both lanes have closed.
    pub(crate) fn forget(&self, agent: &str) -> Result<(), crate::rpc::ErrorCode> {
        let mut database = self
            .database
            .lock()
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        let transaction = database
            .transaction()
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        transaction
            .execute("DELETE FROM side_topics WHERE agent_id=?1", [agent])
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        transaction
            .execute("DELETE FROM side_asks WHERE agent_id=?1", [agent])
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        transaction
            .commit()
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        self.live
            .lock()
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?
            .remove(agent);
        if self.directory.is_some() {
            match std::fs::remove_dir_all(self.directory(agent)?) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(crate::rpc::ErrorCode::AgentIo),
            }
        }
        Ok(())
    }

    /// Share only live metadata between the main and side execution lanes.
    pub(crate) fn live(&self, agent: &str) -> Option<SideQuestionsState> {
        self.live.lock().ok()?.get(agent).cloned()
    }

    /// Publish bounded live metadata after side-topic updates have committed.
    pub(crate) fn publish_live(&self, agent: &str, mut state: SideQuestionsState) {
        state.records.clear();
        if let Ok(mut live) = self.live.lock() {
            live.insert(agent.to_owned(), state);
        }
    }

    /// Return the private auxiliary directory for an Agent, never a native main path.
    pub(crate) fn directory(&self, agent: &str) -> Result<PathBuf, crate::rpc::ErrorCode> {
        let root = self
            .directory
            .as_ref()
            .ok_or(crate::rpc::ErrorCode::AgentIo)?;
        Ok(root.join(format!("{:x}", Sha256::digest(agent.as_bytes()))))
    }

    /// Persist execution strategy and frozen context before any provider send.
    pub(crate) fn prepare_ask(
        &self,
        agent: &str,
        request: &str,
        strategy: SideStrategy,
        snapshot: &SideContextSnapshot,
    ) -> Result<(), crate::rpc::ErrorCode> {
        let snapshot =
            serde_json::to_string(snapshot).map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        self.database
            .lock()
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?
            .execute(
                "UPDATE side_asks SET strategy=?3,snapshot=?4 WHERE agent_id=?1 AND request_id=?2",
                params![agent, request, strategy_name(strategy), snapshot],
            )
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        Ok(())
    }

    /// Whether a different ask still has unresolved acceptance.
    pub(crate) fn uncertain_active(
        &self,
        agent: &str,
        except: &str,
    ) -> Result<bool, crate::rpc::ErrorCode> {
        self.database
            .lock()
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM side_asks WHERE agent_id=?1
                 AND request_id<>?2 AND receipt='uncertain' AND active=1 AND snapshot IS NOT NULL)",
                params![agent, except],
                |row| row.get(0),
            )
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)
    }

    /// Read unresolved sends with the context persisted before execution.
    /// # Errors
    /// Returns storage or corrupted snapshot errors.
    pub(crate) fn pending(
        &self,
        agent: &str,
    ) -> Result<Vec<(String, Option<SideContextSnapshot>)>, crate::rpc::ErrorCode> {
        let database = self
            .database
            .lock()
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        let mut statement = database
            .prepare(
                "SELECT request_id,snapshot FROM side_asks
             WHERE agent_id=?1 AND receipt IN ('uncertain','accepted') AND active=1",
            )
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        let rows = statement
            .query_map([agent], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        rows.map(|row| {
            let (request, snapshot) = row.map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
            let snapshot = snapshot
                .map(|value| serde_json::from_str(&value))
                .transpose()
                .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
            Ok((request, snapshot))
        })
        .collect()
    }

    /// Release the one-active guard only after the runner confirms execution is terminal.
    ///
    /// The uncertain receipt itself is retained: replay never resubmits inference.
    /// # Errors
    /// Returns durable storage failures.
    pub(crate) fn release_uncertain(
        &self,
        agent: &str,
        request: &str,
    ) -> Result<(), crate::rpc::ErrorCode> {
        self.database
            .lock()
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?
            .execute(
                "UPDATE side_asks SET active=0 WHERE agent_id=?1 AND request_id=?2",
                params![agent, request],
            )
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        Ok(())
    }

    /// Reserve an ask key before execution, enforcing idempotent replay.
    ///
    /// # Errors
    /// Returns Agent I/O errors for storage failures.
    pub(crate) fn claim_ask(
        &self,
        agent: &str,
        request_id: &str,
        question: &str,
        record_id: Option<&str>,
    ) -> Result<AskClaim, crate::rpc::ErrorCode> {
        let database = self
            .database
            .lock()
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        if let Some(record) = read_ask(&database, agent, request_id)? {
            if record.question != question || record.record_id.as_deref() != record_id {
                return Ok(AskClaim::Conflict);
            }
            return Ok(match record.receipt {
                SideReceiptState::Accepted => AskClaim::Accepted(record),
                SideReceiptState::Rejected => AskClaim::Rejected(record),
                SideReceiptState::Uncertain => AskClaim::Uncertain(record),
            });
        }
        database
            .execute(
                "INSERT INTO side_asks
                 (agent_id, request_id, topic_id, question, record_id, receipt, strategy, reason, created_at, updated_at)
                 VALUES (?1, ?2, NULL, ?3, ?4, 'uncertain', NULL, NULL, ?5, ?5)",
                params![agent, request_id, question, record_id, now_millis()],
            )
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        Ok(AskClaim::New)
    }

    /// Settle an ask receipt after execution or reconciliation.
    ///
    /// # Errors
    /// Returns Agent I/O errors for storage failures or unknown keys.
    pub(crate) fn settle_ask(
        &self,
        agent: &str,
        request_id: &str,
        response: &SideAskResponse,
        now: u64,
    ) -> Result<(), crate::rpc::ErrorCode> {
        let database = self
            .database
            .lock()
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        let changed = database
            .execute(
                "UPDATE side_asks
                 SET receipt = ?3, topic_id = COALESCE(?4, topic_id),
                     strategy = COALESCE(?5, strategy), reason = ?6, updated_at = ?7,
                     active = CASE WHEN ?3='rejected' THEN 0 ELSE active END
                 WHERE agent_id = ?1 AND request_id = ?2",
                params![
                    agent,
                    request_id,
                    receipt_name(response.receipt),
                    response.record_id,
                    response.strategy.map(strategy_name),
                    response.reason,
                    now
                ],
            )
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        if changed == 0 {
            return Err(crate::rpc::ErrorCode::AgentIo);
        }
        Ok(())
    }

    /// Read one ask receipt.
    ///
    /// # Errors
    /// Returns Agent I/O errors for storage failures.
    pub(crate) fn ask(
        &self,
        agent: &str,
        request_id: &str,
    ) -> Result<Option<AskRecord>, crate::rpc::ErrorCode> {
        let database = self
            .database
            .lock()
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        read_ask(&database, agent, request_id)
    }

    /// Persist the frozen snapshot and initial topic record at first acceptance.
    ///
    /// # Errors
    /// Returns Agent I/O errors for storage failures.
    pub(crate) fn insert_topic(
        &self,
        agent: &str,
        mut topic: SideTopic,
        snapshot: &SideContextSnapshot,
    ) -> Result<(), crate::rpc::ErrorCode> {
        topic.truncated |= snapshot.truncated;
        projection::validate(&mut topic)?;
        let database = self
            .database
            .lock()
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        let snapshot =
            serde_json::to_string(snapshot).map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        let record = serde_json::to_string(&topic).map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        database
            .execute(
                "INSERT INTO side_topics
                 (agent_id, topic_id, strategy, snapshot, record, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    agent,
                    topic.record.id,
                    strategy_name(topic.strategy),
                    snapshot,
                    record,
                    topic.record.created_at,
                    topic.latest_turn().updated_at
                ],
            )
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        Ok(())
    }

    /// Replace the retained topic record after a lifecycle change.
    ///
    /// # Errors
    /// Returns Agent I/O errors for storage failures or unknown topics.
    pub(crate) fn update_topic(
        &self,
        agent: &str,
        mut topic: SideTopic,
    ) -> Result<(), crate::rpc::ErrorCode> {
        projection::validate(&mut topic)?;
        let database = self
            .database
            .lock()
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        let previous: String = database
            .query_row(
                "SELECT record FROM side_topics WHERE agent_id=?1 AND topic_id=?2",
                params![agent, topic.record.id],
                |row| row.get(0),
            )
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        let previous: SideTopic =
            serde_json::from_str(&previous).map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        if !projection::advances(&previous, &topic) {
            return Ok(());
        }
        topic.truncated |= previous.truncated;
        let record = serde_json::to_string(&topic).map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        let changed = database
            .execute(
                "UPDATE side_topics SET record = ?3, updated_at = ?4
                 WHERE agent_id = ?1 AND topic_id = ?2",
                params![
                    agent,
                    topic.record.id,
                    record,
                    topic.latest_turn().updated_at
                ],
            )
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        if changed == 0 {
            return Err(crate::rpc::ErrorCode::AgentIo);
        }
        if topic.latest_turn().status.is_terminal() {
            database
                .execute(
                    "UPDATE side_asks SET active=0 WHERE agent_id=?1 AND topic_id=?2",
                    params![agent, topic.record.id],
                )
                .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        }
        Ok(())
    }

    /// Read one topic with its frozen snapshot.
    ///
    /// # Errors
    /// Returns Agent I/O errors for storage failures.
    pub(crate) fn topic(
        &self,
        agent: &str,
        topic_id: &str,
    ) -> Result<Option<(SideTopic, SideContextSnapshot)>, crate::rpc::ErrorCode> {
        let database = self
            .database
            .lock()
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        database
            .query_row(
                "SELECT record, snapshot FROM side_topics
                 WHERE agent_id = ?1 AND topic_id = ?2",
                params![agent, topic_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?
            .map(|(record, snapshot)| {
                let topic =
                    serde_json::from_str(&record).map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
                let snapshot =
                    serde_json::from_str(&snapshot).map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
                Ok((topic, snapshot))
            })
            .transpose()
    }

    /// Read retained topics newest first.
    ///
    /// # Errors
    /// Returns Agent I/O errors for storage failures.
    pub(crate) fn topics(&self, agent: &str) -> Result<Vec<SideTopic>, crate::rpc::ErrorCode> {
        let database = self
            .database
            .lock()
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        let mut statement = database
            .prepare(
                "SELECT record FROM side_topics WHERE agent_id = ?1
                 ORDER BY created_at DESC, topic_id ASC LIMIT ?2",
            )
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        let rows = statement
            .query_map(params![agent, side_questions::MAX_TOPICS], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
        let mut topics = Vec::new();
        for row in rows {
            let record = row.map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
            let mut topic: SideTopic =
                serde_json::from_str(&record).map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
            projection::bound(&mut topic)?;
            topics.push(topic);
        }
        Ok(topics)
    }

    /// Count retained history without decoding records outside the projection window.
    /// # Errors
    /// Returns storage failures.
    pub(crate) fn topic_count(&self, agent: &str) -> Result<usize, crate::rpc::ErrorCode> {
        self.database
            .lock()
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)?
            .query_row(
                "SELECT count(*) FROM side_topics WHERE agent_id=?1",
                [agent],
                |row| row.get(0),
            )
            .map_err(|_| crate::rpc::ErrorCode::AgentIo)
    }
}

fn read_ask(
    database: &Connection,
    agent: &str,
    request: &str,
) -> Result<Option<AskRecord>, crate::rpc::ErrorCode> {
    database
        .query_row(
            "SELECT receipt,question,record_id,topic_id,strategy,reason
         FROM side_asks WHERE agent_id=?1 AND request_id=?2",
            params![agent, request],
            |row| {
                Ok(AskRecord {
                    receipt: parse_receipt(&row.get::<_, String>(0)?),
                    question: row.get(1)?,
                    record_id: row.get(2)?,
                    topic_id: row.get(3)?,
                    strategy: row
                        .get::<_, Option<String>>(4)?
                        .as_deref()
                        .and_then(parse_strategy),
                    reason: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(|_| crate::rpc::ErrorCode::AgentIo)
}

fn receipt_name(receipt: SideReceiptState) -> &'static str {
    match receipt {
        SideReceiptState::Accepted => "accepted",
        SideReceiptState::Rejected => "rejected",
        SideReceiptState::Uncertain => "uncertain",
    }
}

fn parse_receipt(name: &str) -> SideReceiptState {
    match name {
        "accepted" => SideReceiptState::Accepted,
        "rejected" => SideReceiptState::Rejected,
        _ => SideReceiptState::Uncertain,
    }
}

fn strategy_name(strategy: SideStrategy) -> &'static str {
    match strategy {
        SideStrategy::Native => "native",
        SideStrategy::Isolated => "isolated",
    }
}

fn parse_strategy(name: &str) -> Option<SideStrategy> {
    match name {
        "native" => Some(SideStrategy::Native),
        "isolated" => Some(SideStrategy::Isolated),
        _ => None,
    }
}

/// Capture the frozen display context from the already-committed main projection.
///
/// Reads only committed timeline rows; never forces main-conversation
/// persistence and never dumps live native state. Tool activity is reduced to
/// its displayed summary. The transcript is clipped at [`MAX_SNAPSHOT_BYTES`]
/// with `truncated` set, keeping the newest committed entries.
pub(crate) fn capture_snapshot(
    timeline: &Timeline,
    mut snapshot: SideContextSnapshot,
) -> Result<SideContextSnapshot, crate::rpc::ErrorCode> {
    let overhead =
        side_questions::encoded_size(&snapshot).map_err(|_| crate::rpc::ErrorCode::AgentIo)?;
    let budget = MAX_SNAPSHOT_BYTES
        .checked_sub(overhead)
        .ok_or(crate::rpc::ErrorCode::InvalidMessage)?;
    let (entries, truncated) = timeline.side_context(&snapshot.agent_id, budget)?;
    snapshot.entries = entries;
    snapshot.truncated = truncated;
    Ok(snapshot)
}

/// Scrub credential assignments from already-rendered context while preserving useful URLs.
pub(crate) fn redact_text(text: &str) -> String {
    static CREDENTIALS: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(concat!(
            r#"(?i)(["']?\b(?:authorization|proxy-authorization|api[_ -]?key|"#,
            r#"access[_ -]?token|refresh[_ -]?token|id[_ -]?token|token|"#,
            r#"client[_ -]?secret|secret|password|passwd|cookie|set-cookie)"#,
            r#"["']?\s*[:=]\s*)(?:"[^"\r\n]*"?|'[^'\r\n]*'?|"#,
            r"(?:bearer\s+)?[^\s,;&\)\]\}>]+)",
        ))
        .expect("valid side context credential pattern")
    });
    static USERINFO: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)(https?://)[^/\s@]+@")
            .expect("valid side context URL credential pattern")
    });
    let scrubbed = CREDENTIALS.replace_all(text, "$1[redacted]");
    let mut safe = USERINFO.replace_all(&scrubbed, "$1").into_owned();
    safe.retain(|character| !character.is_control() || matches!(character, '\n' | '\t'));
    safe
}

/// Milliseconds since the Unix epoch; saturates for test clocks.
pub(crate) fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests;
