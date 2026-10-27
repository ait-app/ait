use serde_json::{Value, json};

use crate::local::images::ImageStore;
use crate::ports::agent_session::AgentSessionError;

/// Native text blocks and a separate display projection of the same validated input.
pub(super) struct Prepared {
    pub(super) content: Vec<Value>,
    pub(super) text: String,
    pub(super) sources: Vec<String>,
}

/// Replace image blocks with private file references understood by AGY's native image tools.
/// Returns storage errors before anything is submitted; raw image bytes never enter the timeline.
pub(super) fn prepare(
    mut blocks: Vec<Value>,
    images: &ImageStore,
) -> Result<Prepared, AgentSessionError> {
    let text = blocks
        .iter()
        .filter_map(|block| block["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let mut sources = Vec::new();
    for block in &mut blocks {
        if block["type"] == "image" {
            let source = images.source(block)?.into_owned();
            *block = json!({"type":"text","text":format!("Attached image: {source}")});
            sources.push(source);
        }
    }
    Ok(Prepared {
        content: blocks,
        text,
        sources,
    })
}
