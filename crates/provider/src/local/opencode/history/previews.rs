//! Best-effort discovery previews with bounded time, concurrency and retained user text.
use std::time::Duration;

use domain::agent_runtime::StoredAgentConfig;
use futures_util::{StreamExt, stream};
use serde_json::json;

use super::{OpenCodeClient, SessionDescriptor, launcher};
use crate::{
    local::session_preview, ports::agent_session::AgentSessionError, protocol::timeline::NativeItem,
};

#[derive(Debug, Default)]
struct Prompts {
    current: Option<String>,
    buffer: String,
    first: Option<String>,
    last: Option<String>,
}

impl Prompts {
    fn push(&mut self, id: &str, text: &str) {
        if self.current.as_deref() != Some(id) {
            self.flush();
            self.current = Some(id.to_owned());
        }
        let mut count = (4096 - self.buffer.len()).min(text.len());
        while !text.is_char_boundary(count) {
            count -= 1;
        }
        self.buffer.push_str(&text[..count]);
    }

    fn flush(&mut self) {
        if let Some(text) = session_preview::text(std::iter::once(self.buffer.as_str())) {
            self.first.get_or_insert_with(|| text.clone());
            self.last = Some(text);
        }
        self.buffer.clear();
    }

    fn finish(mut self) -> (Option<String>, Option<String>) {
        self.flush();
        (self.first, self.last)
    }
}

/// Return normalized first/last user previews, joining segments belonging to one message.
pub(super) fn from_entries(entries: &[NativeItem]) -> (Option<String>, Option<String>) {
    let mut prompts = Prompts::default();
    for entry in entries
        .iter()
        .filter(|entry| entry.item["type"] == "user_message")
    {
        if let (Some(id), Some(text)) = (
            entry.item["messageId"].as_str(),
            entry.item["text"].as_str(),
        ) {
            prompts.push(id, text);
        }
    }
    prompts.finish()
}

async fn read(
    client: &OpenCodeClient,
    session: &SessionDescriptor,
) -> Result<(Option<String>, Option<String>), AgentSessionError> {
    let (mut transport, _) =
        launcher::spawn(client, &session.cwd, &StoredAgentConfig::default()).await?;
    let mut prompts = Prompts::default();
    transport
        .request_with_updates(
            "session/load",
            json!({"sessionId":session.provider_handle_id,"cwd":session.cwd,"mcpServers":[]}),
            |message| {
                if message["method"] == "session/update" {
                    if message["params"]["sessionId"] != session.provider_handle_id {
                        return Err(AgentSessionError::Failed);
                    }
                    let update = &message["params"]["update"];
                    if update["sessionUpdate"] == "user_message_chunk"
                        && update["content"]["type"] == "text"
                    {
                        prompts.push(
                            super::config::text(update, "messageId")?,
                            update["content"]["text"]
                                .as_str()
                                .ok_or(AgentSessionError::Failed)?,
                        );
                    }
                }
                Ok(())
            },
        )
        .await?;
    transport.close().await?;
    Ok(prompts.finish())
}

/// Enrich descriptors within a shared deadline; unavailable histories preserve discovery.
pub(in crate::local::opencode) async fn populate(
    client: &OpenCodeClient,
    sessions: &mut [SessionDescriptor],
) {
    let queries = sessions
        .iter()
        .enumerate()
        .map(|(index, session)| {
            let client = client.clone();
            let session = session.clone();
            async move {
                (
                    index,
                    tokio::time::timeout(Duration::from_secs(3), read(&client, &session)).await,
                )
            }
        })
        .collect::<Vec<_>>();
    let mut pending = stream::iter(queries).buffer_unordered(4);
    let mut completed = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some((index, result)) = pending.next().await {
            if let Ok(Ok(previews)) = result {
                completed.push((index, previews));
            }
        }
    })
    .await;
    drop(pending);
    for (index, previews) in completed {
        (
            sessions[index].first_prompt_preview,
            sessions[index].last_prompt_preview,
        ) = previews;
    }
}

#[cfg(test)]
mod tests;
