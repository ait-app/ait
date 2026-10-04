//! Convert native content without exposing bearer URLs or native attachment paths.
use crate::{ports::agent_session::AgentSessionError, protocol::prompt::AgentPrompt};
use serde_json::{Value, json};

pub(super) fn prompt(prompt: &AgentPrompt) -> Result<Vec<Value>, AgentSessionError> {
    prompt.blocks()?.iter().map(|block| match block["type"].as_str() {
        Some("text") => Ok(block.clone()),
        Some("image") => Ok(json!({"type":"image","mediaType":block["mimeType"],"data":block["data"]})),
        Some("resource_link") => Ok(json!({"type":"text","text":format!("[{}]({})",block["name"].as_str().unwrap_or("attachment"),block["uri"].as_str().ok_or(AgentSessionError::Rejected)?)})),
        _ => Err(AgentSessionError::Rejected),
    }).collect()
}

pub(super) fn has_images(frame: &Value) -> bool {
    blocks(frame).is_some_and(|blocks| {
        blocks.iter().any(|block| {
            block["type"] == "image"
                || block["type"] == "tool-result"
                    && block["content"]
                        .as_array()
                        .is_some_and(|content| content.iter().any(|item| item["type"] == "image"))
        })
    })
}

fn blocks(frame: &Value) -> Option<&Vec<Value>> {
    (frame["streamId"] == "history" && frame["value"]["type"] == "event")
        .then(|| frame["value"]["event"]["data"]["message"]["content"].as_array())
        .flatten()
}

pub(super) async fn hydrate(
    mut frame: Value,
    api: &super::http::Api,
    session: &str,
    images: &crate::local::images::ImageStore,
) -> Result<Value, AgentSessionError> {
    if let Some(blocks) = frame["value"]["event"]["data"]["message"]["content"].as_array_mut() {
        for block in blocks {
            if block["type"] == "tool-result" {
                if let Some(content) = block["content"].as_array_mut() {
                    for item in content {
                        hydrate_image(item, api, session, images).await?;
                    }
                }
            } else {
                hydrate_image(block, api, session, images).await?;
            }
        }
    }
    Ok(frame)
}

async fn hydrate_image(
    block: &mut Value,
    api: &super::http::Api,
    session: &str,
    images: &crate::local::images::ImageStore,
) -> Result<(), AgentSessionError> {
    if block["type"] != "image" {
        return Ok(());
    }
    let attachment = &block["attachment"];
    let id = attachment["attachmentId"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or(AgentSessionError::Failed)?;
    let result = api
        .call(
            "session/attachment",
            json!({"request":{"sessionId":session,"attachmentId":id}}),
        )
        .await?;
    if result["attachment"] != *attachment {
        return Err(AgentSessionError::Failed);
    }
    let text = images.render(&json!({"mimeType":attachment["mediaType"],"data":result["data"]}))?;
    *block = json!({"type":"text","text":text});
    Ok(())
}
