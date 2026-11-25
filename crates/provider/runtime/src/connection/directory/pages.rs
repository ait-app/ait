//! Pages of a single synchronized Agent directory read, owned by its connection.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use model::ErrorCode;
use serde_json::{Value, json};
use uuid::Uuid;

// Reserve space for the RPC envelope, correlation ID, subscription ID and page cursor.
const PAGE_BYTES: usize = model::server::MAX_MESSAGE_BYTES - 4096;
const CACHE_BYTES: usize = 64 * 1024 * 1024;
const IDLE_LIMIT: Duration = Duration::from_secs(120);
const PREFIX: &str = "agent-sync:";

#[derive(Debug, Default)]
pub(crate) struct Pages {
    reads: BTreeMap<Uuid, Read>,
}

#[derive(Debug)]
struct Read {
    remaining: Vec<Vec<u8>>,
    size: usize,
    touched: Instant,
    observed: bool,
    sort: Value,
    limit: usize,
    subscription: Option<String>,
    completed: bool,
}

impl Pages {
    pub(crate) fn release(&mut self, id: &str) {
        self.reads
            .retain(|_, read| read.subscription.as_deref() != Some(id));
    }

    /// Preserve the reconciled result while the caller traverses it with ordinary list requests.
    pub(crate) fn first(
        &mut self,
        params: &Value,
        mut response: Value,
    ) -> Result<Value, ErrorCode> {
        if response["sync"].is_null() {
            return Ok(response);
        }
        let limit = limit(params)?;
        let entries = take_array(&mut response["entries"])?;
        let removals = take_array(&mut response["sync"]["removals"])?;
        let subscription = response["subscriptionId"].as_str().map(str::to_owned);
        let id = Uuid::new_v4();
        let mut rows = entries
            .into_iter()
            .map(|row| (true, row))
            .chain(removals.into_iter().map(|row| (false, row)))
            .peekable();
        let mut scratch = Vec::new();
        let mut build_page = |mut page: Value, index: usize| -> Result<Value, ErrorCode> {
            let mut entries = Vec::new();
            let mut removals = Vec::new();
            let mut bytes = 0;
            for _ in 0..limit {
                let Some((_, row)) = rows.peek() else { break };
                scratch.clear();
                serde_json::to_writer(&mut scratch, row).map_err(|_| ErrorCode::AgentIo)?;
                let length = scratch.len() + 1;
                if length > PAGE_BYTES {
                    return Err(ErrorCode::ResourceExhausted);
                }
                if bytes + length > PAGE_BYTES {
                    break;
                }
                bytes += length;
                let (upsert, row) = rows.next().expect("peeked directory row");
                if upsert {
                    entries.push(row);
                } else {
                    removals.push(row);
                }
            }
            let more = rows.peek().is_some();
            let next = more.then(|| format!("{PREFIX}{id}:{index}"));
            page["entries"] = Value::Array(entries);
            page["sync"]["removals"] = Value::Array(removals);
            page["pageInfo"] = json!({"nextCursor":next,"prevCursor":null,"hasMore":more});
            Ok(page)
        };
        let first = build_page(response.clone(), 0)?;
        if first["pageInfo"]["hasMore"] == false {
            return Ok(first);
        }
        if let Some(object) = response.as_object_mut() {
            object.remove("subscriptionId");
        }
        let mut read = Read {
            remaining: Vec::new(),
            size: 0,
            touched: Instant::now(),
            limit,
            subscription,
            observed: params["includeObserved"].as_bool().unwrap_or(false),
            sort: params["sort"].clone(),
            completed: false,
        };
        loop {
            let page = build_page(response.clone(), read.remaining.len() + 1)?;
            let encoded = serde_json::to_vec(&page).map_err(|_| ErrorCode::AgentIo)?;
            read.size += encoded.len();
            if read.size > CACHE_BYTES {
                return Err(ErrorCode::ResourceExhausted);
            }
            read.remaining.push(encoded);
            if page["pageInfo"]["hasMore"] == false {
                break;
            }
        }
        self.retain_read(id, read)?;
        Ok(first)
    }

    fn retain_read(&mut self, id: Uuid, read: Read) -> Result<(), ErrorCode> {
        let fits = |reads: &BTreeMap<Uuid, Read>| {
            read.size + reads.values().map(|value| value.size).sum::<usize>() <= CACHE_BYTES
        };
        self.reads
            .retain(|_, value| value.touched.elapsed() < IDLE_LIMIT);
        if !fits(&self.reads) {
            self.reads.retain(|_, value| !value.completed);
            if !fits(&self.reads) {
                return Err(ErrorCode::ResourceExhausted);
            }
        }
        self.reads.insert(id, read);
        Ok(())
    }

    pub(crate) fn resume(&mut self, params: &Value) -> Option<Result<Value, ErrorCode>> {
        let cursor = params["page"]["cursor"].as_str()?.strip_prefix(PREFIX)?;
        Some(self.page(params, cursor))
    }

    fn page(&mut self, params: &Value, cursor: &str) -> Result<Value, ErrorCode> {
        let (id, index) = cursor.split_once(':').ok_or(ErrorCode::InvalidMessage)?;
        let id = Uuid::parse_str(id).map_err(|_| ErrorCode::InvalidMessage)?;
        let index: usize = index.parse().map_err(|_| ErrorCode::InvalidMessage)?;
        let read = self.reads.get_mut(&id).ok_or(ErrorCode::InvalidMessage)?;
        if read.touched.elapsed() >= IDLE_LIMIT
            || !params["subscribe"].is_null()
            || !params["filter"].is_null()
            || params["scope"] != "active"
            || read.observed != params["includeObserved"].as_bool().unwrap_or(false)
            || read.sort != params["sort"]
            || read.limit != limit(params)?
        {
            return Err(ErrorCode::InvalidMessage);
        }
        let encoded = read.remaining.get(index).ok_or(ErrorCode::InvalidMessage)?;
        read.touched = Instant::now();
        read.completed |= index + 1 == read.remaining.len();
        serde_json::from_slice(encoded).map_err(|_| ErrorCode::AgentIo)
    }
}

fn take_array(value: &mut Value) -> Result<Vec<Value>, ErrorCode> {
    match value.take() {
        Value::Array(rows) => Ok(rows),
        _ => Err(ErrorCode::AgentIo),
    }
}

fn limit(params: &Value) -> Result<usize, ErrorCode> {
    let count = params["page"]["limit"].as_u64().unwrap_or(200);
    usize::try_from(count)
        .ok()
        .filter(|limit| (1..=200).contains(limit))
        .ok_or(ErrorCode::InvalidMessage)
}

#[cfg(test)]
mod tests;
