use domain::bot::{Channel, ChannelConfig};
use model::ErrorCode;
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::{Incoming, chunks, deliver, expose, json_response};
use crate::service::Bots;

const API: &str = "https://api.telegram.org";

pub(super) async fn run(
    bots: &Bots,
    channel: &Channel,
    client: &reqwest::Client,
    token: &SecretString,
    cancel: &CancellationToken,
) -> Result<(), ErrorCode> {
    let me = call(client, token, "getMe", json!({})).await?;
    let identity = me["id"].as_i64().ok_or(ErrorCode::BotIo)?;
    let username = me["username"].as_str().ok_or(ErrorCode::BotIo)?;
    bots.account(channel.id, format!("telegram:{identity}"))
        .await?;
    let webhook = call(client, token, "getWebhookInfo", json!({})).await?;
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
        api: API,
    };
    let receiving = receiver.receive(cancel);
    let sending = deliver(bots, channel.id, cancel, |turn, conversation| async move {
        let text = turn.reply.as_deref().ok_or(ErrorCode::BotIo)?;
        for chunk in chunks(text, 3500) {
            let mut body = json!({"chat_id":conversation.peer,"text":chunk});
            if let Some(message) = &turn.reply_to {
                body["reply_parameters"] =
                    json!({"message_id":message.parse::<i64>().map_err(|_| ErrorCode::BotIo)?});
            }
            if let Some(thread) = &conversation.thread {
                body["message_thread_id"] =
                    json!(thread.parse::<i64>().map_err(|_| ErrorCode::BotIo)?);
            }
            call(client, token, "sendMessage", body).await?;
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
                {
                    self.bots.accept(incoming).await?;
                }
                cursor = id.checked_add(1).ok_or(ErrorCode::BotIo)?;
                self.bots.cursor(self.channel.id, cursor).await?;
            }
        }
    }
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
    let Some(text) = message["text"]
        .as_str()
        .filter(|text| !text.trim().is_empty() && text.len() <= 16 * 1024)
    else {
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

async fn call(
    client: &reqwest::Client,
    token: &SecretString,
    method: &str,
    body: Value,
) -> Result<Value, ErrorCode> {
    call_at(client, token, method, body, API).await
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
            tokio::time::sleep(std::time::Duration::from_secs(seconds.min(60))).await;
            continue;
        }
        // The response may contain a token-bearing URL or personal data; discard it.
        return Err(ErrorCode::BotIo);
    }
    Err(ErrorCode::BotIo)
}

#[cfg(test)]
mod tests;
