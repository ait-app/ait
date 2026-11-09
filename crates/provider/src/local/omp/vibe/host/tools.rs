use serde_json::{Value, json};

/// Public host-tool definitions; execution and argument validation belong to the Rust coordinator.
pub(super) fn definitions() -> Vec<Value> {
    [
        ("spawn", "Start a persistent fast/good worker; returns immediately", json!({"cli":{"type":"string","enum":["fast","good"]},"prompt":{"type":"string","minLength":1,"maxLength":65536},"name":{"type":"string","maxLength":48}}), vec!["cli", "prompt"]),
        ("send", "Send a follow-up or steer a running worker; returns immediately", json!({"session":{"type":"string"},"message":{"type":"string","minLength":1,"maxLength":65536}}), vec!["session", "message"]),
        ("wait", "Wait for a worker turn to finish and read its answer", json!({"sessions":{"type":"array","items":{"type":"string"},"maxItems":32},"timeout":{"type":"number","exclusiveMinimum":0,"maximum":600}}), vec![]),
        ("kill", "Cancel and close an owned worker session", json!({"session":{"type":"string"}}), vec!["session"]),
        ("list", "Read owned worker status and bounded answers", json!({}), vec![]),
    ].into_iter().map(|(operation,description,properties,required)| json!({
        "name":format!("vibe_{operation}"),"description":description,"loadMode":"essential",
        "parameters":{"type":"object","properties":properties,"required":required,"additionalProperties":false}
    })).collect()
}

/// A normal OMP host-tool result, also used for explicit native-action receipts.
pub(super) fn result(details: &Value, error: bool) -> Value {
    let result = json!({"content":[{"type":"text","text":details.to_string()}],"details":details,"isError":error});
    if serde_json::to_vec(&result).is_ok_and(|bytes| bytes.len() <= 900 * 1024) {
        result
    } else {
        json!({"content":[{"type":"text","text":"Result exceeds the RPC budget; query individual workers with vibe_wait."}],"isError":true})
    }
}
