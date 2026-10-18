//! User image display projection shared by live completion and native history replay.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::local::images::ImageStore;
use crate::ports::agent_session::AgentSessionError;

const LAYOUT_HEADER: &str = "Image layout: overviews show the full image; detail crops preserve readable pixels and overlap. Crop coordinates refer to the top-left of the original image.";

/// Project a native user message into text and one preview source per original image.
/// # Errors
/// Returns a malformed native content error; unavailable images retain a placeholder.
pub(super) fn project(native: &Value, images: &ImageStore) -> Result<Value, AgentSessionError> {
    let content = native["content"]
        .as_array()
        .ok_or(AgentSessionError::Failed)?;
    let image_count = content.iter().filter(|part| is_image(part)).count();
    let layout = content.iter().enumerate().rev().find_map(|(index, part)| {
        hidden_crops(part["text"].as_str()?, image_count).map(|hidden| (index, hidden))
    });
    let mut text = Vec::new();
    let mut sources = Vec::new();
    let mut image_index = 0;
    for (index, part) in content.iter().enumerate() {
        if layout.as_ref().is_some_and(|(note, _)| *note == index) {
            continue;
        }
        if is_image(part) {
            image_index += 1;
            if layout
                .as_ref()
                .is_some_and(|(_, hidden)| hidden.contains(&image_index))
            {
                continue;
            }
            match images.source(part) {
                Ok(source) => sources.push(source),
                // Older native histories can omit the image URL entirely.
                Err(_) => text.push("[Image attachment]"),
            }
        } else if let Some(value) = part["text"].as_str() {
            text.push(value);
        }
    }
    let mut item = json!({"type":"user_message","text":text.join("\n"),"messageId":native["id"]});
    if let Some(client) = native["clientId"].as_str() {
        item["clientMessageId"] = json!(client);
    }
    if !sources.is_empty() {
        item["imageSources"] = json!(sources);
    }
    Ok(item)
}

fn is_image(part: &Value) -> bool {
    matches!(part["type"].as_str(), Some("image" | "localImage"))
}

/// Recognize only the complete generated layout grammar and matching image positions.
fn hidden_crops(text: &str, image_count: usize) -> Option<BTreeSet<usize>> {
    let lines = text
        .strip_prefix(LAYOUT_HEADER)?
        .strip_prefix('\n')?
        .lines();
    let mut hidden = BTreeSet::new();
    let mut previous_index = 0;
    let mut previous_original = 0;
    let mut has_detail = false;
    for line in lines {
        let (index, description) = line
            .strip_prefix("Image ")?
            .split_once(": original image ")?;
        let index = index.parse::<usize>().ok()?;
        let (original, region) = description.split_once(": ")?;
        let original = original.parse::<usize>().ok()?;
        if index <= previous_index || index > image_count || original == 0 || original > index {
            return None;
        }
        if original > previous_original {
            if previous_original > 0 && !has_detail {
                return None;
            }
            let dimensions = region
                .strip_prefix("overview of the full ")?
                .strip_suffix(" image")?;
            let (width, height) = dimensions.split_once('x')?;
            if width.parse::<u32>().ok()? == 0 || height.parse::<u32>().ok()? == 0 {
                return None;
            }
            has_detail = false;
        } else if original == previous_original && index == previous_index + 1 {
            let region = region
                .strip_prefix("detail crop x=")?
                .strip_suffix("; original pixel resolution")?;
            let (x, rest) = region.split_once(", y=")?;
            let (y, rest) = rest.split_once(", width=")?;
            let (width, height) = rest.split_once(", height=")?;
            x.parse::<u32>().ok()?;
            y.parse::<u32>().ok()?;
            if width.parse::<u32>().ok()? == 0 || height.parse::<u32>().ok()? == 0 {
                return None;
            }
            hidden.insert(index);
            has_detail = true;
        } else {
            return None;
        }
        previous_index = index;
        previous_original = original;
    }
    has_detail.then_some(hidden)
}

#[cfg(test)]
mod tests;
