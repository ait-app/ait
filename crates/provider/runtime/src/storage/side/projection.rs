//! Shared bounded BTW records and monotonic lifecycle fencing.

use super::redact_text;
use model::ErrorCode;
use provider_core::protocol::side_questions::{
    MAX_ANSWER_BYTES, MAX_ERROR_BYTES, MAX_QUESTION_BYTES, MAX_RECORD_BYTES, MAX_TURNS, SideTopic,
    bounded, encoded_size,
};

/// Whether an observed record can advance the retained topic without resurrecting a turn.
pub(super) fn advances(previous: &SideTopic, incoming: &SideTopic) -> bool {
    if previous == incoming
        || previous.record.id != incoming.record.id
        || previous.strategy != incoming.strategy
    {
        return false;
    }
    let old_count = previous.record.turn_count();
    let new_count = incoming.record.turn_count();
    if new_count != old_count {
        return new_count > old_count;
    }
    let old = previous.latest_turn();
    let new = incoming.latest_turn();
    old.created_at == new.created_at
        && !old.status.is_terminal()
        && new.updated_at >= old.updated_at
        && (new.status.is_terminal() || new.answer.len() >= old.answer.len())
}

/// Explicit durable-storage budget; history is never silently dropped to fit the wire projection.
const MAX_DURABLE_TOPIC_BYTES: usize = 16 * 1024 * 1024;

/// Validate full history before storage without dropping any questions or answers.
/// # Errors
/// Returns invalid-message for malformed identity or exhausted durable budget.
pub(super) fn validate(topic: &mut SideTopic) -> Result<(), ErrorCode> {
    validate_identity(topic)?;
    if encoded_size(topic).map_err(|_| ErrorCode::AgentIo)? > MAX_DURABLE_TOPIC_BYTES {
        return Err(ErrorCode::InvalidMessage);
    }
    for error in topic.record.error.iter_mut().chain(
        topic
            .record
            .follow_ups
            .iter_mut()
            .filter_map(|turn| turn.error.as_mut()),
    ) {
        *error = redact_text(error);
    }
    Ok(())
}

fn validate_identity(topic: &SideTopic) -> Result<(), ErrorCode> {
    let id = topic.record.id.as_bytes();
    if id.is_empty()
        || id.len() > 128
        || !id[0].is_ascii_alphanumeric()
        || !id
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(ErrorCode::InvalidMessage);
    }
    Ok(())
}

/// Bound the owned record in place, preserving native identity and the latest turn.
/// # Errors
/// Returns invalid-message for malformed native identity, or serialization failures.
pub(super) fn bound(topic: &mut SideTopic) -> Result<(), ErrorCode> {
    validate_identity(topic)?;
    let record = &mut topic.record;
    if record.follow_ups.len() >= MAX_TURNS {
        record
            .follow_ups
            .drain(..record.follow_ups.len() - (MAX_TURNS - 1));
        topic.truncated = true;
    }
    clip(
        &mut record.question,
        MAX_QUESTION_BYTES,
        &mut topic.truncated,
    );
    clip(&mut record.answer, MAX_ANSWER_BYTES, &mut topic.truncated);
    sanitize_error(&mut record.error, &mut topic.truncated);
    if let Some(title) = &mut record.title {
        clip(title, MAX_QUESTION_BYTES, &mut topic.truncated);
    }
    for turn in &mut record.follow_ups {
        clip(&mut turn.question, MAX_QUESTION_BYTES, &mut topic.truncated);
        clip(&mut turn.answer, MAX_ANSWER_BYTES, &mut topic.truncated);
        sanitize_error(&mut turn.error, &mut topic.truncated);
    }
    if encoded_size(topic).map_err(|_| ErrorCode::AgentIo)? <= MAX_RECORD_BYTES {
        return Ok(());
    }
    topic.truncated = true;
    topic.record.images.clear();
    for turn in &mut topic.record.follow_ups {
        turn.images.clear();
    }
    let mut size = encoded_size(topic).map_err(|_| ErrorCode::AgentIo)?;
    let record = &mut topic.record;
    // Older answers yield first, keeping the latest answer useful under the total budget.
    for text in std::iter::once(&mut record.answer)
        .chain(record.follow_ups.iter_mut().map(|turn| &mut turn.answer))
    {
        shrink(text, &mut size)?;
    }
    for text in std::iter::once(&mut record.question)
        .chain(record.follow_ups.iter_mut().map(|turn| &mut turn.question))
        .chain(record.title.iter_mut())
        .chain(record.error.iter_mut())
    {
        shrink(text, &mut size)?;
    }
    for error in record
        .follow_ups
        .iter_mut()
        .filter_map(|turn| turn.error.as_mut())
    {
        shrink(error, &mut size)?;
    }
    if size > MAX_RECORD_BYTES {
        return Err(ErrorCode::InvalidMessage);
    }
    Ok(())
}

fn clip(text: &mut String, limit: usize, truncated: &mut bool) {
    let (prefix, clipped) = bounded(text, limit);
    let end = prefix.len();
    if clipped {
        text.truncate(end);
        *truncated = true;
    }
}

fn sanitize_error(error: &mut Option<String>, truncated: &mut bool) {
    if let Some(text) = error {
        *text = redact_text(text);
        clip(text, MAX_ERROR_BYTES, truncated);
    }
}

fn shrink(text: &mut String, total: &mut usize) -> Result<(), ErrorCode> {
    let excess = total.saturating_sub(MAX_RECORD_BYTES);
    if excess == 0 || text.is_empty() {
        return Ok(());
    }
    let before = encoded_size(text).map_err(|_| ErrorCode::AgentIo)?;
    let end = text.floor_char_boundary(text.len().saturating_sub(excess));
    text.truncate(end);
    let after = encoded_size(text).map_err(|_| ErrorCode::AgentIo)?;
    *total -= before - after;
    Ok(())
}

#[cfg(test)]
mod tests;
