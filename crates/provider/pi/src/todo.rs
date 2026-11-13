//! The official Pi example extension stores its complete task list in result details.

use serde_json::{Value, json};

pub(super) fn snapshot(result: &Value) -> Option<Value> {
    let details = &result["details"];
    if details.get("error").is_some_and(|error| !error.is_null()) {
        return None;
    }
    let todos = details["todos"]
        .as_array()
        .filter(|todos| todos.len() <= 1024)?;
    let mut entries = Vec::with_capacity(todos.len());
    let mut text_bytes = 0_usize;
    for todo in todos {
        todo["id"].as_u64()?;
        let text = todo["text"].as_str().filter(|text| text.len() <= 8192)?;
        text_bytes += text.len();
        if text_bytes > 128 * 1024 {
            return None;
        }
        let done = todo["done"].as_bool()?;
        entries.push(json!({"content":text,"status":if done {"completed"} else {"pending"}}));
    }
    let mut item = provider_native::todo::snapshot(&Value::Array(entries)).ok()?;
    for (item, todo) in item["items"].as_array_mut()?.iter_mut().zip(todos) {
        item["id"] = json!(todo["id"].as_u64()?.to_string());
    }
    (serde_json::to_vec(&item).ok()?.len() <= 128 * 1024).then_some(item)
}

#[cfg(test)]
mod tests;
