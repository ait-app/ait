//! Timeline query semantics over immutable Provider display rows.

/// Client methods implemented by this component; consumed by capability discovery.
pub(crate) const METHODS: &[&str] = &[
    "agent.timeline.get.request",
    "agent.timeline.search.request",
    "agent.timeline.list_prompts.request",
    "agent.timeline.append.request",
    "agent.timeline.set_subscription.request",
];

use model::ErrorCode;
use serde_json::{Value, json};

use crate::protocol::timeline::{Direction, FetchRequest, SearchRequest};
use crate::storage::timeline::Row;

pub(super) mod projection;
mod text_search;

const MAX_RESPONSE_BYTES: usize = 900 * 1024;

pub(crate) fn fetch(
    request: &FetchRequest,
    epoch: &str,
    rows: &[Row],
    agent: &Value,
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
    let page = projection::select(
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
    if request.merge_window == Some(true) {
        value["mergeWindow"] = json!(true);
    }
    let envelope_bytes = serde_json::to_vec(&value)
        .map_err(|_| ErrorCode::AgentIo)?
        .len();
    // A narrower source window can increase the digit count of either cursor's sequence.
    let entry_bytes = MAX_RESPONSE_BYTES.saturating_sub(envelope_bytes + 40);
    let page = projection::fit(rows, page, selection, entry_bytes)?;
    value["entries"] = serde_json::to_value(page.entries).map_err(|_| ErrorCode::AgentIo)?;
    value["startCursor"] = page
        .start_seq
        .map_or(Value::Null, |seq| json!({"epoch":epoch,"seq":seq}));
    value["endCursor"] = page
        .end_seq
        .map_or(Value::Null, |seq| json!({"epoch":epoch,"seq":seq}));
    value["hasOlder"] = json!(page.has_older);
    value["hasNewer"] = json!(page.has_newer);
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
    let messages = projection::project(rows);
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
