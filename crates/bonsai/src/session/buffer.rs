//! The coalescing window: events gathered for up to 150 ms are merged before numbering, so
//! replay and live delivery see the same sequence.

use crate::event::{Body, TEXT_CHUNK_BYTES};

/// Pending, not yet numbered events of one run.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Buffer {
    bodies: Vec<Body>,
}

impl Buffer {
    /// Whether nothing is pending.
    pub(crate) fn is_empty(&self) -> bool {
        self.bodies.is_empty()
    }

    /// Take everything pending, in order.
    pub(crate) fn take(&mut self) -> Vec<Body> {
        std::mem::take(&mut self.bodies)
    }

    /// Add an event, merging it into a pending one where the contract allows.
    pub(crate) fn push(&mut self, body: Body) {
        match body {
            Body::Text { mid, text } => {
                if let Some(Body::Text {
                    mid: last_mid,
                    text: last,
                }) = self.bodies.last_mut()
                    && *last_mid == mid
                    && fits(last, &text)
                {
                    last.push_str(&text);
                    return;
                }
                self.bodies.push(Body::Text { mid, text });
            }
            Body::Reasoning { mid, text } => {
                if let Some(Body::Reasoning {
                    mid: last_mid,
                    text: last,
                }) = self.bodies.last_mut()
                    && *last_mid == mid
                    && fits(last, &text)
                {
                    last.push_str(&text);
                    return;
                }
                self.bodies.push(Body::Reasoning { mid, text });
            }
            Body::Tool { .. } => self.push_tool(body),
            Body::Usage { .. } => {
                self.bodies
                    .retain(|pending| !matches!(pending, Body::Usage { .. }));
                self.bodies.push(body);
            }
            other => self.bodies.push(other),
        }
    }

    fn push_tool(&mut self, body: Body) {
        let Body::Tool { id, .. } = &body else {
            return;
        };
        let existing = self.bodies.iter_mut().find(
            |pending| matches!(pending, Body::Tool { id: pending_id, .. } if pending_id == id),
        );
        match existing {
            Some(existing) => merge_tool(existing, body),
            None => self.bodies.push(body),
        }
    }
}

fn fits(current: &str, addition: &str) -> bool {
    let escaped = |text: &str| serde_json::to_string(text).map_or(usize::MAX, |json| json.len());
    escaped(current).saturating_add(escaped(addition)) <= TEXT_CHUNK_BYTES
}

/// Overlay a newer snapshot on an older pending one: present fields win.
fn merge_tool(existing: &mut Body, newer: Body) {
    let (
        Body::Tool {
            name,
            state,
            kind,
            title,
            input,
            output,
            truncated,
            ..
        },
        Body::Tool {
            name: new_name,
            state: new_state,
            kind: new_kind,
            title: new_title,
            input: new_input,
            output: new_output,
            truncated: new_truncated,
            ..
        },
    ) = (existing, newer)
    else {
        return;
    };
    *name = new_name;
    *state = new_state;
    if new_kind.is_some() {
        *kind = new_kind;
    }
    if new_title.is_some() {
        *title = new_title;
    }
    if new_input.is_some() {
        *input = new_input;
    }
    if new_output.is_some() {
        *output = new_output;
    }
    if new_truncated.is_some() {
        *truncated = new_truncated;
    }
}

#[cfg(test)]
mod tests;
