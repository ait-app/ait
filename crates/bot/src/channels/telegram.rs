use std::time::Duration;

use domain::bot::{Channel, ChannelConfig};
use model::ErrorCode;
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::{
    BUSY_NOTICE, Disposition, Incoming, admit, chunks, deliver, expose, json_response, reply_parts,
};
use crate::service::Bots;

pub(crate) mod onboarding;

const API: &str = "https://api.telegram.org";
/// Longest wait between admission attempts while the Bot's global backlog is full.
const MAX_BACKLOG_RETRY: Duration = Duration::from_secs(30);
const PROCESSING_REACTION: &str = "\u{1f440}";

pub(super) async fn run(
    bots: &Bots,
    channel: &Channel,
    client: &reqwest::Client,
    token: &SecretString,
    cancel: &CancellationToken,
) -> Result<(), ErrorCode> {
    run_at(bots, channel, (client, token, API), cancel).await
}

async fn run_at(
    bots: &Bots,
    channel: &Channel,
    transport: (&reqwest::Client, &SecretString, &str),
    cancel: &CancellationToken,
) -> Result<(), ErrorCode> {
    let (client, token, api) = transport;
    let me = call_at(client, token, "getMe", json!({}), api).await?;
    let identity = me["id"].as_i64().ok_or(ErrorCode::BotIo)?;
    let username = me["username"].as_str().ok_or(ErrorCode::BotIo)?;
    bots.account(channel.id, format!("telegram:{identity}"))
        .await?;
    let webhook = call_at(client, token, "getWebhookInfo", json!({}), api).await?;
    if webhook["url"].as_str().is_none_or(|url| !url.is_empty()) {
        bots.channel_status(channel.id, json!({"state":"conflict","error":"This Telegram account already has a webhook; remove it explicitly before enabling polling"})).await;
        cancel.cancelled().await;
        return Ok(());
    }
    bots.channel_status(
        channel.id,
        json!({"state":"connected","accountId":identity.to_string()}),
    )
    .await;
    let receiver = Receiver {
        bots,
        channel,
        client,
        token,
        identity,
        username,
        api,
    };
    let receiving = receiver.receive(cancel);
    let sending = deliver(bots, channel.id, cancel, |turn, conversation| async move {
        let parts = reply_parts(&turn);
        if parts.is_empty() {
            return Err(ErrorCode::BotIo);
        }
        for text in parts {
            for chunk in chunks(text, 3500) {
                let body = message_body(
                    &conversation.peer,
                    conversation.thread.as_deref(),
                    turn.reply_to.as_deref(),
                    chunk,
                )?;
                call_at(client, token, "sendMessage", body, api).await?;
            }
        }
        Ok(())
    });
    tokio::select! {
        () = cancel.cancelled() => Ok(()),
        result = receiving => result,
        result = sending => result,
    }
}

struct Receiver<'a> {
    bots: &'a Bots,
    channel: &'a Channel,
    client: &'a reqwest::Client,
    token: &'a SecretString,
    identity: i64,
    username: &'a str,
    api: &'a str,
}
impl Receiver<'_> {
    async fn receive(&self, cancel: &CancellationToken) -> Result<(), ErrorCode> {
        let mut cursor = self.channel.cursor.unwrap_or(0);
        loop {
            let updates = tokio::select! {
                () = cancel.cancelled() => return Ok(()),
                result = call_at(self.client, self.token, "getUpdates", json!({"offset":cursor,"timeout":30,"limit":50,"allowed_updates":["message"]}), self.api) => result?,
            };
            for update in updates.as_array().ok_or(ErrorCode::BotIo)? {
                let id = update["update_id"].as_i64().ok_or(ErrorCode::BotIo)?;
                if id < cursor {
                    continue;
                }
                if let Some(incoming) =
                    incoming(self.channel, update, self.identity, self.username)?
                    && !self.admit(incoming, cancel).await?
                {
                    return Ok(());
                }
                cursor = id.checked_add(1).ok_or(ErrorCode::BotIo)?;
                self.bots.cursor(self.channel.id, cursor).await?;
            }
        }
    }

    /// Admit one update before its offset is committed; returns `false` when cancelled first.
    /// A full Bot backlog is waited out without advancing the offset, while a saturated
    /// conversation drops only its own message so other chats keep flowing.
    async fn admit(
        &self,
        incoming: Incoming,
        cancel: &CancellationToken,
    ) -> Result<bool, ErrorCode> {
        let mut backoff = Duration::from_secs(1);
        loop {
            match admit(self.bots, incoming.clone()).await? {
                Disposition::Accepted => {
                    self.react(&incoming).await;
                    return Ok(true);
                }
                Disposition::Skip => return Ok(true),
                Disposition::Busy => {
                    self.notify_busy(&incoming).await;
                    return Ok(true);
                }
                Disposition::Retry => {
                    tokio::select! {
                        () = cancel.cancelled() => return Ok(false),
                        () = tokio::time::sleep(backoff) => {}
                    }
                    backoff = (backoff * 2).min(MAX_BACKLOG_RETRY);
                }
            }
        }
    }

    /// Best-effort reaction showing that a durably admitted message is being processed.
    async fn react(&self, incoming: &Incoming) {
        let Some(message_id) = incoming.reply_to.as_deref() else {
            return;
        };
        let result = react(
            self.client,
            self.token,
            &incoming.peer,
            message_id,
            self.api,
        )
        .await;
        if let Err(error) = result {
            tracing::warn!(channel = %self.channel.id, ?error, "Telegram reaction failed");
        }
    }

    /// Best-effort reply telling the sender their message was dropped; failures are only logged.
    async fn notify_busy(&self, incoming: &Incoming) {
        let sent = match message_body(
            &incoming.peer,
            incoming.thread.as_deref(),
            incoming.reply_to.as_deref(),
            BUSY_NOTICE,
        ) {
            Ok(body) => call_at(self.client, self.token, "sendMessage", body, self.api)
                .await
                .map(|_| ()),
            Err(error) => Err(error),
        };
        if let Err(error) = sent {
            tracing::warn!(channel = %self.channel.id, ?error, "Telegram busy notice failed");
        }
    }
}

/// Build a `sendMessage` body that replies in the original chat, topic and message thread.
fn message_body(
    peer: &str,
    thread: Option<&str>,
    reply_to: Option<&str>,
    text: &str,
) -> Result<Value, ErrorCode> {
    let mut body = json!({"chat_id":peer,"text":text});
    if let Some(message) = reply_to {
        body["reply_parameters"] =
            json!({"message_id":message.parse::<i64>().map_err(|_| ErrorCode::BotIo)?});
    }
    if let Some(thread) = thread {
        body["message_thread_id"] = json!(thread.parse::<i64>().map_err(|_| ErrorCode::BotIo)?);
    }
    Ok(body)
}

async fn react(
    client: &reqwest::Client,
    token: &SecretString,
    chat_id: &str,
    message_id: &str,
    api: &str,
) -> Result<(), ErrorCode> {
    let message_id = message_id.parse::<i64>().map_err(|_| ErrorCode::BotIo)?;
    call_at(
        client,
        token,
        "setMessageReaction",
        json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "reaction": [{"type": "emoji", "emoji": PROCESSING_REACTION}],
            "is_big": false,
        }),
        api,
    )
    .await
    .map(|_| ())
}

fn incoming(
    channel: &Channel,
    update: &Value,
    identity: i64,
    username: &str,
) -> Result<Option<Incoming>, ErrorCode> {
    let ChannelConfig::Telegram {
        allowed_senders,
        allow_groups,
        ..
    } = &channel.config
    else {
        return Err(ErrorCode::InvalidMessage);
    };
    let message = &update["message"];
    let Some(sender) = message["from"]["id"].as_i64() else {
        return Ok(None);
    };
    let sender = sender.to_string();
    if message["from"]["is_bot"] == true || !allowed_senders.contains(&sender) {
        return Ok(None);
    }
    let Some(text) = message["text"].as_str().filter(|text| {
        !text.trim().is_empty() && text.len() <= 16 * 1024 && !onboarding::is_pairing_start(text)
    }) else {
        return Ok(None);
    };
    if message["chat"]["type"] != "private"
        && (!allow_groups
            || (!mentioned(text, username)
                && message["reply_to_message"]["from"]["id"].as_i64() != Some(identity)))
    {
        return Ok(None);
    }
    Ok(Some(Incoming {
        channel_id: channel.id,
        peer: message["chat"]["id"]
            .as_i64()
            .ok_or(ErrorCode::BotIo)?
            .to_string(),
        sender,
        thread: message["message_thread_id"]
            .as_i64()
            .map(|id| id.to_string()),
        message_id: update["update_id"]
            .as_i64()
            .ok_or(ErrorCode::BotIo)?
            .to_string(),
        text: text.to_owned(),
        reply_to: message["message_id"].as_i64().map(|id| id.to_string()),
    }))
}

fn mentioned(text: &str, username: &str) -> bool {
    let mention = format!("@{}", username.to_ascii_lowercase());
    let text = text.to_ascii_lowercase();
    text.match_indices(&mention).any(|(offset, _)| {
        text[offset + mention.len()..]
            .chars()
            .next()
            .is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_')
    })
}

async fn call_at(
    client: &reqwest::Client,
    token: &SecretString,
    method: &str,
    body: Value,
    api: &str,
) -> Result<Value, ErrorCode> {
    let endpoint = format!("{api}/bot{}/{method}", expose(token));
    for attempt in 0..3 {
        let response = client
            .post(&endpoint)
            .json(&body)
            .send()
            .await
            .map_err(|_| ErrorCode::BotIo)?;
        let value = json_response(response).await?;
        if value["ok"] == true {
            return Ok(value["result"].clone());
        }
        if attempt < 2
            && value["error_code"] == 429
            && let Some(seconds) = value["parameters"]["retry_after"].as_u64()
        {
            tokio::time::sleep(Duration::from_secs(seconds.min(60))).await;
            continue;
        }
        // The response may contain a token-bearing URL or personal data; discard it.
        return Err(ErrorCode::BotIo);
    }
    Err(ErrorCode::BotIo)
}

#[cfg(test)]
mod tests;
