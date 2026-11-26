use serde_json::json;

use provider_core::{ports::agent_session::AgentTurnEvent, protocol::timeline::NativeItem};

/// Split accepted input into durable rows within the timeline's escaped JSON budget.
/// Preserves text, attachment previews and client identity.
pub(super) fn timeline(
    conversation: &str,
    turn: &str,
    text: &str,
    sources: &[String],
    client_message_id: Option<&str>,
) -> Vec<AgentTurnEvent> {
    // A control byte can occupy six JSON bytes; leave space for timeline metadata.
    const INPUT_TEXT_BYTES: usize = 96 * 1024;
    let timestamp = chrono::Utc::now().to_rfc3339();
    let mut remaining = text;
    let mut events = Vec::new();
    loop {
        let end = remaining.floor_char_boundary(INPUT_TEXT_BYTES.min(remaining.len()));
        let key = format!(
            "native:antigravity:{conversation}:{turn}:user:{}",
            events.len()
        );
        let mut item = json!({"type":"user_message","messageId":key,"text":&remaining[..end]});
        if events.is_empty() {
            if let Some(id) = client_message_id {
                item["clientMessageId"] = json!(id);
            }
            if !sources.is_empty() {
                item["imageSources"] = json!(sources);
            }
        }
        events.push(AgentTurnEvent::Timeline(NativeItem {
            key,
            turn_id: Some(turn.to_owned()),
            timestamp: timestamp.clone(),
            item,
        }));
        remaining = &remaining[end..];
        if remaining.is_empty() {
            return events;
        }
    }
}
