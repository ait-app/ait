mod feishu;
mod telegram;

use std::sync::Arc;
use std::time::Duration;

use domain::ChannelId;
use domain::bot::{Channel, ChannelConfig, DeliveryStatus};
use model::ErrorCode;
use secrecy::SecretString;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::ports::Credentials;
use crate::service::Bots;

#[derive(Debug)]
pub(crate) struct Incoming {
    pub(crate) channel_id: ChannelId,
    pub(crate) peer: String,
    pub(crate) sender: String,
    pub(crate) thread: Option<String>,
    pub(crate) message_id: String,
    pub(crate) text: String,
    pub(crate) reply_to: Option<String>,
}

pub(crate) async fn run(
    bots: Bots,
    channel: Channel,
    credentials: Arc<dyn Credentials>,
    cancel: CancellationToken,
) {
    let Ok(client) = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(45))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    else {
        bots.channel_status(
            channel.id,
            json!({"state":"error","error":"HTTP transport unavailable"}),
        )
        .await;
        return;
    };
    let mut backoff = 1;
    loop {
        let result = attempt(&bots, &channel, &credentials, &client, &cancel).await;
        if cancel.is_cancelled() {
            return;
        }
        bots.channel_status(
            channel.id,
            json!({"state":"reconnecting","error":result.err().map(ErrorCode::message)}),
        )
        .await;
        tokio::select! {
            () = cancel.cancelled() => return,
            () = tokio::time::sleep(Duration::from_secs(backoff)) => {}
        }
        backoff = (backoff * 2).min(60);
    }
}

async fn attempt(
    bots: &Bots,
    channel: &Channel,
    credentials: &Arc<dyn Credentials>,
    client: &reqwest::Client,
    cancel: &CancellationToken,
) -> Result<(), ErrorCode> {
    let reference = match &channel.config {
        ChannelConfig::Builtin => return Ok(()),
        ChannelConfig::Telegram { token_ref, .. } => token_ref,
        ChannelConfig::Feishu { secret_ref, .. } => secret_ref,
    };
    let secret = credentials.resolve(reference)?;
    match &channel.config {
        ChannelConfig::Builtin => Ok(()),
        ChannelConfig::Telegram { .. } => {
            telegram::run(bots, channel, client, &secret, cancel).await
        }
        ChannelConfig::Feishu { .. } => feishu::run(bots, channel, client, &secret, cancel).await,
    }
}

async fn deliver<F, Fut>(
    bots: &Bots,
    channel: ChannelId,
    cancel: &CancellationToken,
    send: F,
) -> Result<(), ErrorCode>
where
    F: Fn(domain::bot::Turn, domain::bot::Conversation) -> Fut,
    Fut: Future<Output = Result<(), ErrorCode>>,
{
    loop {
        let Some((turn, conversation)) = bots.delivery_next(channel).await? else {
            tokio::select! {
                () = cancel.cancelled() => return Ok(()),
                () = tokio::time::sleep(Duration::from_millis(500)) => {}
            }
            continue;
        };
        let id = turn.id;
        let outcome = tokio::select! {
            () = cancel.cancelled() => Err(ErrorCode::BotIo),
            result = send(turn, conversation) => result,
        };
        bots.delivery(
            id,
            if outcome.is_ok() {
                DeliveryStatus::Delivered
            } else {
                DeliveryStatus::Uncertain
            },
        )
        .await?;
        if cancel.is_cancelled() {
            return Ok(());
        }
    }
}

fn chunks(text: &str, limit: usize) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut count = 0;
    for (offset, character) in text.char_indices() {
        let size = character.len_utf16();
        if count + size > limit {
            result.push(&text[start..offset]);
            start = offset;
            count = 0;
        }
        count += size;
    }
    if start < text.len() {
        result.push(&text[start..]);
    }
    result
}

async fn json_response(response: reqwest::Response) -> Result<serde_json::Value, ErrorCode> {
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| ErrorCode::BotIo)? {
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err(ErrorCode::ResourceExhausted);
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| ErrorCode::BotIo)
}

use secrecy::ExposeSecret;

fn expose(secret: &SecretString) -> &str {
    secret.expose_secret()
}

#[cfg(test)]
mod tests;
