pub(crate) mod feishu;
pub(crate) mod github;
mod onboarding;
pub(crate) mod photon;
pub(crate) mod qq;
pub(crate) mod telegram;
pub(crate) mod webhook;
pub(crate) mod weixin;

use std::sync::Arc;
use std::time::Duration;

use domain::ChannelId;
use domain::bot::{Channel, ChannelConfig, DeliveryStatus, TurnContext};
use model::ErrorCode;
use secrecy::SecretString;
use serde_json::json;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_util::sync::CancellationToken;

use crate::ports::Credentials;
use crate::service::{Admission, Bots};

/// Reply sent when a conversation's own backlog refuses a message, which is then dropped.
const BUSY_NOTICE: &str =
    "This conversation is busy with earlier messages; please resend this one later.";
/// Longest wait between admission attempts while the Bot's global backlog is full.
const MAX_BACKLOG_RETRY: Duration = Duration::from_secs(30);

/// What a receiver does with one inbound message after asking the Bot to admit it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Disposition {
    /// Durably queued; acknowledge it.
    Accepted,
    /// The global backlog is full; leave it unacknowledged so it is delivered again.
    Retry,
    /// It can never be admitted; acknowledge and drop it.
    Skip,
    /// Its conversation is saturated; acknowledge and drop it, then tell the sender.
    Busy,
}

/// Admit one channel message and classify the outcome identically for every channel.
/// A saturated conversation never blocks other chats; a saturated Bot service never loses input.
async fn admit(bots: &Bots, incoming: Incoming) -> Result<Disposition, ErrorCode> {
    let (channel, message) = (incoming.channel_id, incoming.message_id.clone());
    match bots.accept(incoming).await {
        Ok(Admission::Accepted) => Ok(Disposition::Accepted),
        Ok(Admission::Backlogged) => {
            tracing::warn!(%channel, message, "Bot backlog is full; channel input will be retried");
            Ok(Disposition::Retry)
        }
        Err(error @ (ErrorCode::InvalidMessage | ErrorCode::IdempotencyConflict)) => {
            tracing::warn!(%channel, message, ?error, "skipping channel input refused by the Bot");
            Ok(Disposition::Skip)
        }
        Err(ErrorCode::ResourceExhausted) => {
            tracing::warn!(%channel, message, "dropping channel input for a saturated conversation");
            Ok(Disposition::Busy)
        }
        Err(error) => Err(error),
    }
}

/// Admit one message, waiting out a full Bot backlog; the result is never `Retry`.
/// Callers run inside a cancellable `select!`, which drops this future on shutdown.
async fn admit_waiting(bots: &Bots, incoming: Incoming) -> Result<Disposition, ErrorCode> {
    let mut backoff = Duration::from_secs(1);
    loop {
        match admit(bots, incoming.clone()).await? {
            Disposition::Retry => {
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKLOG_RETRY);
            }
            settled => return Ok(settled),
        }
    }
}

/// Run a receiver beside its delivery loop, which must watch `stop`. Ending delivery that way
/// records an interrupted send as uncertain; dropping it would strand the turn as sending.
async fn supervise<R, S>(
    cancel: &CancellationToken,
    stop: &CancellationToken,
    receiving: R,
    sending: S,
) -> Result<(), ErrorCode>
where
    R: Future<Output = Result<(), ErrorCode>>,
    S: Future<Output = Result<(), ErrorCode>>,
{
    tokio::pin!(sending);
    let received = tokio::select! {
        () = cancel.cancelled() => Ok(()),
        result = receiving => result,
        result = &mut sending => return result,
    };
    stop.cancel();
    sending.await?;
    received
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Open a platform WebSocket with the frame limits every channel shares.
async fn connect_socket<R>(endpoint: R) -> Result<Socket, ErrorCode>
where
    R: tokio_tungstenite::tungstenite::client::IntoClientRequest + Unpin,
{
    tokio_tungstenite::connect_async_with_config(
        endpoint,
        Some(
            WebSocketConfig::default()
                .max_message_size(Some(1024 * 1024))
                .max_frame_size(Some(1024 * 1024)),
        ),
        false,
    )
    .await
    .map(|(socket, _)| socket)
    .map_err(|_| ErrorCode::BotIo)
}

#[derive(Clone, Debug)]
pub(crate) struct Incoming {
    pub(crate) channel_id: ChannelId,
    pub(crate) peer: String,
    pub(crate) sender: String,
    pub(crate) thread: Option<String>,
    pub(crate) message_id: String,
    pub(crate) text: String,
    pub(crate) reply_to: Option<String>,
    pub(crate) context: Option<TurnContext>,
}

pub(crate) async fn github_webhook(
    channel: &Channel,
    credentials: &Arc<dyn Credentials>,
    client: &reqwest::Client,
    event: &str,
    delivery_id: &str,
    signature: &str,
    body: &[u8],
) -> Result<Option<Incoming>, ErrorCode> {
    github::webhook(
        channel,
        credentials,
        client,
        event,
        delivery_id,
        signature,
        body,
    )
    .await
}

/// Return each user-visible assistant message for external delivery.
///
/// New turns carry distinct messages; the legacy reply projection remains a fallback for records
/// written by older daemons.
pub(crate) fn reply_parts(turn: &domain::bot::Turn) -> Vec<&str> {
    if turn.messages.is_empty() {
        return turn.reply.as_deref().into_iter().collect();
    }
    turn.messages
        .iter()
        .filter(|message| !message.text.trim().is_empty())
        .map(|message| message.text.as_str())
        .collect()
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
    if matches!(&channel.config, ChannelConfig::Github { .. }) {
        return github::run(
            bots.clone(),
            channel.clone(),
            credentials.clone(),
            client.clone(),
            cancel,
        )
        .await;
    }
    let reference = match &channel.config {
        ChannelConfig::Builtin | ChannelConfig::Webhook { .. } => {
            return Ok(());
        }
        ChannelConfig::Telegram { token_ref, .. } | ChannelConfig::Weixin { token_ref, .. } => {
            token_ref
        }
        ChannelConfig::Feishu { secret_ref, .. } | ChannelConfig::Qq { secret_ref, .. } => {
            secret_ref
        }
        ChannelConfig::PhotonImessage { key_ref, .. } => key_ref,
        ChannelConfig::Github { .. } => unreachable!("GitHub handled before credential lookup"),
    };
    let secret = credentials.resolve(reference)?;
    match &channel.config {
        ChannelConfig::Builtin | ChannelConfig::Webhook { .. } => Ok(()),
        ChannelConfig::Telegram { .. } => {
            telegram::run(bots, channel, client, &secret, cancel).await
        }
        ChannelConfig::Feishu { .. } => feishu::run(bots, channel, client, &secret, cancel).await,
        ChannelConfig::Weixin { .. } => weixin::run(bots, channel, client, &secret, cancel).await,
        ChannelConfig::Qq { .. } => qq::run(bots, channel, client, &secret, cancel).await,
        ChannelConfig::PhotonImessage { .. } => {
            photon::run(bots, channel, client, &secret, cancel).await
        }
        ChannelConfig::Github { .. } => unreachable!("GitHub handled before credential lookup"),
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
