//! Native command images and Agent-scoped staged file receipts.

use serde_json::{Value, json};
use tokio::fs::File;

use super::super::http::Api;
use crate::{ports::agent_session::AgentSessionError, protocol::prompt::AgentPrompt};

#[derive(Debug)]
pub(in crate::local::deepseek_harness::native) enum Attachment {
    Image(Value),
    File {
        file: File,
        name: String,
        bytes: u64,
    },
}

/// Validate every attachment and open regular files before starting any native uploads.
/// Rejects unsupported context objects, missing files, changed sizes, and oversized files.
pub(in crate::local::deepseek_harness::native) async fn prepare(
    prompt: &AgentPrompt,
) -> Result<Vec<Attachment>, AgentSessionError> {
    prompt.validate()?;
    if prompt
        .attachments
        .iter()
        .any(|attachment| attachment["type"] != "uploaded_file")
    {
        return Err(AgentSessionError::Rejected);
    }
    let mut attachments = Vec::with_capacity(prompt.images.len() + prompt.attachments.len());
    for image in &prompt.images {
        attachments.push(Attachment::Image(json!({"type":"image",
            "mediaType":image.mime_type,"data":image.data})));
    }
    for attachment in &prompt.attachments {
        let bytes = attachment["size"]
            .as_u64()
            .filter(|size| *size <= 256 * 1024 * 1024)
            .ok_or(AgentSessionError::Rejected)?;
        let name = attachment["fileName"]
            .as_str()
            .filter(|name| name.len() <= 1024)
            .ok_or(AgentSessionError::Rejected)?
            .to_owned();
        let path = attachment["path"]
            .as_str()
            .ok_or(AgentSessionError::Rejected)?;
        let file = File::open(path)
            .await
            .map_err(|_| AgentSessionError::Rejected)?;
        let metadata = file
            .metadata()
            .await
            .map_err(|_| AgentSessionError::Rejected)?;
        if !metadata.is_file() || metadata.len() != bytes {
            return Err(AgentSessionError::Rejected);
        }
        attachments.push(Attachment::File { file, name, bytes });
    }
    Ok(attachments)
}

/// Upload exact file bytes for `agent` and retain native receipt authority in submission order.
/// Propagates storage or transport failures; never submits a partially prepared command.
pub(in crate::local::deepseek_harness::native) async fn submit(
    api: &Api,
    agent: &str,
    attachments: Vec<Attachment>,
) -> Result<Vec<Value>, AgentSessionError> {
    let mut submitted = Vec::with_capacity(attachments.len());
    for attachment in attachments {
        submitted.push(match attachment {
            Attachment::Image(image) => image,
            Attachment::File { file, name, bytes } => {
                let receipt = api.upload_file(agent, &name, file, bytes).await?;
                json!({"type":"file","receiptId":receipt})
            }
        });
    }
    Ok(submitted)
}
