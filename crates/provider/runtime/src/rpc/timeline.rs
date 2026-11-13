//! Timeline query semantics over immutable Provider display rows.

/// Client methods implemented by this component; consumed by capability discovery.
pub(crate) const METHODS: &[MethodSpec] = &[
    MethodSpec::request("agent.timeline.get.request"),
    MethodSpec::request("agent.timeline.search.request"),
    MethodSpec::request("agent.timeline.list_prompts.request"),
    MethodSpec::request("agent.timeline.append.request"),
    MethodSpec::request("agent.timeline.set_subscription.request"),
];

use model::ErrorCode;
use model::methods::MethodSpec;
use serde_json::{Value, json};

#[cfg(test)]
use crate::protocol::timeline::Direction;
use crate::protocol::timeline::{FetchRequest, SearchRequest};
use crate::storage::timeline::Row;
use crate::storage::timeline::Timeline;
#[cfg(test)]
use crate::timeline_projection::page;
use crate::timeline_projection::project;

mod text_search;

const MAX_RESPONSE_BYTES: usize = 900 * 1024;

#[derive(Clone, Copy)]
enum TaskEncoding {
    Full,
    Reference,
    Omit,
}

/// Fetch `request` from the indexed `timeline` scope with `agent` metadata in the wire envelope.
/// Optional `history_needs_refresh` is reserved in the response byte budget before selecting rows.
/// Returns a projected response or storage, cursor validation and response-budget errors.
pub(crate) fn fetch_stored(
    request: &FetchRequest,
    timeline: &Timeline,
    scope: &str,
    agent: &Value,
    history_needs_refresh: Option<bool>,
) -> Result<Value, ErrorCode> {
    with_task_budget(|encoding| {
        fetch_stored_with_tasks(
            request,
            timeline,
            scope,
            (agent, history_needs_refresh),
            encoding,
        )
    })
}

fn fetch_stored_with_tasks(
    request: &FetchRequest,
    timeline: &Timeline,
    scope: &str,
    metadata: (&Value, Option<bool>),
    encoding: TaskEncoding,
) -> Result<Value, ErrorCode> {
    let (agent, history_needs_refresh) = metadata;
    let mut value = Value::Null;
    let page = timeline.page(request, scope, |header, task_snapshot| {
        let reset = header.stale || header.gap;
        value = json!({"agentId":request.agent_id,"agent":agent,"direction":header.direction,
            "projection":"projected","epoch":header.epoch,"reset":reset,
            "staleCursor":header.stale,"gap":header.gap,
            "window":{"minSeq":header.min_seq,"maxSeq":header.max_seq,
                "nextSeq":header.max_seq.saturating_add(1)},
            "startCursor":{"epoch":header.epoch,"seq":header.max_seq},
            "endCursor":{"epoch":header.epoch,"seq":header.max_seq},
            "hasOlder":false,"hasNewer":false,"entries":[],"error":null});
        if let Some(snapshot) = encoded_tasks(task_snapshot, encoding) {
            value["taskSnapshot"] = snapshot;
        }
        if request.merge_window == Some(true) {
            value["mergeWindow"] = json!(true);
        }
        if let Some(history_needs_refresh) = history_needs_refresh {
            value["historyNeedsRefresh"] = json!(history_needs_refresh);
        }
        let envelope = serde_json::to_vec(&value)
            .map_err(|_| ErrorCode::AgentIo)?
            .len();
        Ok(MAX_RESPONSE_BYTES.saturating_sub(envelope + 40))
    })?;
    let epoch = value["epoch"].clone();
    value["entries"] = serde_json::to_value(page.entries).map_err(|_| ErrorCode::AgentIo)?;
    value["startCursor"] = page
        .start_seq
        .map_or(Value::Null, |seq| json!({"epoch":epoch,"seq":seq}));
    value["endCursor"] = page
        .end_seq
        .map_or(Value::Null, |seq| json!({"epoch":epoch,"seq":seq}));
    value["hasOlder"] = json!(page.has_older);
    value["hasNewer"] = json!(page.has_newer);
    bounded_task_page(value)
}

#[cfg(test)]
pub(crate) fn fetch(
    request: &FetchRequest,
    epoch: &str,
    rows: &[Row],
    agent: &Value,
) -> Result<Value, ErrorCode> {
    with_task_budget(|encoding| fetch_with_tasks(request, epoch, rows, agent, encoding))
}

#[cfg(test)]
fn fetch_with_tasks(
    request: &FetchRequest,
    epoch: &str,
    rows: &[Row],
    agent: &Value,
    encoding: TaskEncoding,
) -> Result<Value, ErrorCode> {
    let direction = request.direction.unwrap_or(if request.cursor.is_some() {
        Direction::After
    } else {
        Direction::Tail
    });
    let stale = request
        .cursor
        .as_ref()
        .is_some_and(|cursor| cursor.epoch != epoch);
    let next = rows.last().map_or(1, |row| row.seq.saturating_add(1));
    let gap = direction == Direction::After
        && request.cursor.as_ref().is_some_and(|cursor| {
            !stale
                && rows
                    .first()
                    .is_some_and(|first| cursor.seq < first.seq.saturating_sub(1))
        });
    let reset = stale || gap;
    let limit = request.limit.unwrap_or(if direction == Direction::After {
        0
    } else {
        200
    });
    let selection = if reset { Direction::Tail } else { direction };
    let page = page::select(
        rows,
        selection,
        request.cursor.as_ref().map(|cursor| cursor.seq),
        limit,
    );
    let mut value = json!({"agentId":request.agent_id,"agent":agent,"direction":direction,
        "projection":"projected","epoch":epoch,"reset":reset,"staleCursor":stale,"gap":gap,
        "window":{"minSeq":rows.first().map_or(0, |row| row.seq),"maxSeq":next.saturating_sub(1),"nextSeq":next},
        "startCursor":page.start_seq.map(|seq|json!({"epoch":epoch,"seq":seq})),
        "endCursor":page.end_seq.map(|seq|json!({"epoch":epoch,"seq":seq})),
        "hasOlder":page.has_older,"hasNewer":page.has_newer,
        "entries":[],"error":null});
    let task_snapshot = rows
        .iter()
        .rev()
        .find(|row| row.entry.item["type"] == "todo")
        .map_or(
            Value::Null,
            |row| json!({"epoch":epoch,"seq":row.seq,"items":row.entry.item["items"]}),
        );
    if let Some(snapshot) = encoded_tasks(task_snapshot, encoding) {
        value["taskSnapshot"] = snapshot;
    }
    if request.merge_window == Some(true) {
        value["mergeWindow"] = json!(true);
    }
    let envelope_bytes = serde_json::to_vec(&value)
        .map_err(|_| ErrorCode::AgentIo)?
        .len();
    // A narrower source window can increase the digit count of either cursor's sequence.
    let entry_bytes = MAX_RESPONSE_BYTES.saturating_sub(envelope_bytes + 40);
    let page = page::fit(rows, page, selection, entry_bytes)?;
    value["entries"] = serde_json::to_value(page.entries).map_err(|_| ErrorCode::AgentIo)?;
    value["startCursor"] = page
        .start_seq
        .map_or(Value::Null, |seq| json!({"epoch":epoch,"seq":seq}));
    value["endCursor"] = page
        .end_seq
        .map_or(Value::Null, |seq| json!({"epoch":epoch,"seq":seq}));
    value["hasOlder"] = json!(page.has_older);
    value["hasNewer"] = json!(page.has_newer);
    bounded_task_page(value)
}

// Large legacy task lists must not be duplicated past the existing page budget. A reference
// carries the latest list only when that exact canonical row is in the returned page. If the
// list and an unrelated large row cannot fit together, preserve ordinary history pagination.
fn with_task_budget(
    mut fetch: impl FnMut(TaskEncoding) -> Result<Value, ErrorCode>,
) -> Result<Value, ErrorCode> {
    for encoding in [
        TaskEncoding::Full,
        TaskEncoding::Reference,
        TaskEncoding::Omit,
    ] {
        match fetch(encoding) {
            Ok(value) => return Ok(value),
            Err(ErrorCode::ResourceExhausted) => {}
            Err(error) => return Err(error),
        }
    }
    Err(ErrorCode::ResourceExhausted)
}

fn encoded_tasks(snapshot: Value, encoding: TaskEncoding) -> Option<Value> {
    match encoding {
        TaskEncoding::Full => Some(snapshot),
        TaskEncoding::Reference if snapshot.is_null() => Some(Value::Null),
        TaskEncoding::Reference => Some(json!({
            "epoch":snapshot["epoch"],"seq":snapshot["seq"],"entrySeq":snapshot["seq"]
        })),
        TaskEncoding::Omit => None,
    }
}

fn bounded_task_page(value: Value) -> Result<Value, ErrorCode> {
    if let Some(seq) = value["taskSnapshot"]["entrySeq"].as_u64()
        && !value["entries"].as_array().is_some_and(|entries| {
            entries
                .iter()
                .any(|entry| entry["seqStart"] == seq && entry["item"]["type"] == "todo")
        })
    {
        return Err(ErrorCode::ResourceExhausted);
    }
    bounded(value)
}

pub(crate) fn search(
    request: &SearchRequest,
    epoch: &str,
    rows: &[Row],
) -> Result<Value, ErrorCode> {
    let Some(pattern) = text_search::pattern(&request.query)? else {
        return bounded(json!({"agentId":request.agent_id,"epoch":epoch,
            "locations":[],"nextCursor":null,"error":null}));
    };
    let offset = request.cursor.unwrap_or(0);
    let messages = project(rows);
    let mut matching = messages
        .iter()
        .filter(|row| row.seq_end > offset as u64)
        .filter_map(|row| {
            let role = item_role(&row.item)?;
            let text = row.item["text"].as_str()?;
            let count = text_search::count(&pattern, text, role == "assistant");
            (count > 0).then(|| json!({"seq":row.seq_end,"role":role,"count":count}))
        });
    let locations: Vec<_> = matching.by_ref().take(200).collect();
    let next = matching
        .next()
        .and_then(|_| locations.last().map(|location| location["seq"].clone()));
    bounded(
        json!({"agentId":request.agent_id,"epoch":epoch,"locations":locations,"nextCursor":next,"error":null}),
    )
}

pub(crate) fn prompts(agent: &str, epoch: &str, rows: &[Row]) -> Result<Value, ErrorCode> {
    let prompts: Vec<_> = rows
        .iter()
        .filter(|row| role(row) == Some("user"))
        .map(|row| {
            let text = row.entry.item["text"].as_str().unwrap_or("");
            let preview = preview(text);
            json!({"seq":row.seq,"timestamp":row.entry.timestamp,"preview":preview})
        })
        .collect();
    bounded(json!({"agentId":agent,"epoch":epoch,"prompts":prompts,"error":null}))
}

fn preview(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.encode_utf16().count() <= 120 {
        return collapsed;
    }
    let mut units = 0;
    let mut preview: String = collapsed
        .chars()
        .take_while(|character| {
            units += character.len_utf16();
            units <= 119
        })
        .collect();
    preview.push('…');
    preview
}

fn role(row: &Row) -> Option<&'static str> {
    item_role(&row.entry.item)
}

fn item_role(item: &Value) -> Option<&'static str> {
    match item["type"].as_str() {
        Some("user_message") => Some("user"),
        Some("assistant_message") => Some("assistant"),
        _ => None,
    }
}

pub(crate) fn bounded(value: Value) -> Result<Value, ErrorCode> {
    if serde_json::to_vec(&value)
        .map_err(|_| ErrorCode::AgentIo)?
        .len()
        > MAX_RESPONSE_BYTES
    {
        return Err(ErrorCode::ResourceExhausted);
    }
    Ok(value)
}

#[cfg(test)]
mod tests;
