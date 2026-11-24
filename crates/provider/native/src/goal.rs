//! Bounded native Goal facts shared by adapters and projected through runtime info.

use serde_json::{Value, json};

/// Normalize a native goal or goal-mode envelope for the composer track.
///
/// `editable` declares whether the adapter can update the objective without replacing the goal.
/// Returns `Some(null)` for an authoritative clear, and `None` for malformed native data.
#[must_use]
pub fn snapshot(native: &Value, editable: bool) -> Option<Value> {
    let goal = native.get("goal").unwrap_or(native);
    if goal.is_null() || goal["status"] == "dropped" {
        return Some(Value::Null);
    }
    let objective = goal["objective"].as_str()?.trim();
    if objective.is_empty() || objective.len() > 65_536 {
        return None;
    }
    let status = match goal["status"].as_str()? {
        "active" => "active",
        "paused" => "paused",
        "blocked" => "blocked",
        "usageLimited" => "usageLimited",
        "complete" | "completed" => "completed",
        "budgetLimited" | "budget_limited" | "budget_exhausted" => "budgetLimited",
        "failed" => "failed",
        _ => "unknown",
    };
    let mut value = json!({"objective":objective,"status":status,"actions":{
        "edit":editable,"pause":status == "active",
        "resume":matches!(status,"paused" | "budgetLimited" | "blocked" | "usageLimited"),"clear":true
    }});
    for key in ["tokenBudget", "tokensUsed", "timeUsedSeconds"] {
        if let Some(number) = goal[key].as_u64() {
            value[key] = json!(number);
        }
    }
    Some(value)
}

#[cfg(test)]
mod tests;
