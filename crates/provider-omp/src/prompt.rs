//! OMP `prompt` and `steer` payloads built from ordered host prompt blocks (D7).

use serde_json::{Value, json};

/// One input as OMP receives it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Payload {
    /// Message text: text blocks joined by a blank line.
    pub(super) text: String,
    /// Images, in order.
    images: Vec<Value>,
}

impl Payload {
    /// Split ordered `blocks` into text and images. Every image goes to OMP whatever the
    /// current model accepts: for a text-only model OMP 18.18.8 saves the image itself and
    /// tells the model so in a hidden `image-attachment-description` message.
    pub(super) fn new(blocks: Vec<Value>) -> Self {
        let mut text = String::new();
        let mut images = Vec::new();
        for block in blocks {
            if block["type"] == "image" {
                images.push(block);
            } else if let Some(part) = block["text"].as_str() {
                if !text.is_empty() {
                    text.push_str("\n\n");
                }
                text.push_str(part);
            }
        }
        Self { text, images }
    }

    /// OMP's request fields: `message`, `images` only when there are any (as Paseo omits an
    /// empty list), and `streamingBehavior` for a queued `/steer` or `/follow-up`.
    pub(super) fn fields(self, streaming_behavior: Option<&str>) -> Value {
        let mut fields = json!({"message":self.text});
        if !self.images.is_empty() {
            fields["images"] = Value::Array(self.images);
        }
        if let Some(behavior) = streaming_behavior {
            fields["streamingBehavior"] = json!(behavior);
        }
        fields
    }
}

#[cfg(test)]
mod tests;
