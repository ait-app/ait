//! Ait Vibe orchestration over the installed OMP public RPC and extension APIs.
pub(super) mod host;

fn preview(value: &serde_json::Value, limit: usize) -> String {
    value
        .as_str()
        .unwrap_or_default()
        .chars()
        .take(limit)
        .collect()
}
