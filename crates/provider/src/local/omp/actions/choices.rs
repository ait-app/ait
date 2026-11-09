use serde_json::{Value, json};

const MAX_CHOICES: usize = 128;
const MAX_ENCODED_BYTES: usize = 24 * 1024;

/// Build recent history choices without truncating a serialized JSON document.
///
/// `value` is the owned native `get_entries` response. Returns valid bounded JSON, including
/// omitted-entry counts, or `None` when the response has no entry array or cannot be serialized.
/// Full messages remain in the native session; this receipt only supplies object selectors.
pub(super) fn history(value: &Value) -> Option<String> {
    let entries = value["entries"].as_array()?;
    let mut choices = Vec::with_capacity(entries.len().min(MAX_CHOICES));
    let mut encoded_bytes = 0;
    for entry in entries.iter().rev() {
        if choices.len() == MAX_CHOICES {
            break;
        }
        let Some(id) = entry["id"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control))
        else {
            continue;
        };
        let choice = json!({"id":id,"label":label(entry),"role":entry["message"]["role"].as_str().filter(|role| role.len() <= 32),
            "entryType":entry["type"].as_str().filter(|kind| kind.len() <= 64)});
        let bytes = serde_json::to_vec(&choice).ok()?.len() + 1;
        if encoded_bytes + bytes > MAX_ENCODED_BYTES {
            break;
        }
        encoded_bytes += bytes;
        choices.push(choice);
    }
    choices.reverse();
    let leaf_id = value["leafId"].as_str().filter(|id| id.len() <= 256);
    serde_json::to_string(
        &json!({"omitted":entries.len() - choices.len(),"entries":choices,
        "leafId":leaf_id}),
    )
    .ok()
}

fn label(entry: &Value) -> String {
    let message = &entry["message"];
    let content = &message["content"];
    let text = entry["label"]
        .as_str()
        .filter(|label| !label.trim().is_empty())
        .or_else(|| {
            if message["role"] == "toolResult" {
                return message["toolName"].as_str();
            }
            content.as_str().or_else(|| {
                content.as_array()?.iter().find_map(|block| {
                    if block["type"] == "text" {
                        block["text"]
                            .as_str()
                            .filter(|text| !text.trim().is_empty())
                    } else {
                        None
                    }
                })
            })
        })
        .or_else(|| entry["type"].as_str())
        .unwrap_or("History entry");
    text.chars()
        .take(120)
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
