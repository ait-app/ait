//! Native message text and image attachments derived from ordered host prompt blocks.

use serde_json::Value;

/// Join the text of ordered prompt `blocks`, separating blocks with a blank line.
pub(super) fn text(blocks: &[Value]) -> String {
    blocks
        .iter()
        .filter_map(|block| block["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Split ordered prompt `blocks` into joined message text and their image blocks.
pub(super) fn split(blocks: Vec<Value>) -> (String, Vec<Value>) {
    let text = text(&blocks);
    let images = blocks
        .into_iter()
        .filter(|block| block["type"] == "image")
        .collect();
    (text, images)
}

#[cfg(test)]
mod tests;
