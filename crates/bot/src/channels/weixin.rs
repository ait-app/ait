//! `WeChat` channel: private chats with an `iLink` bot over long polling; see ADR-225.

pub(crate) mod onboarding;

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use domain::bot::{Channel, ChannelConfig, Conversation, Turn};
use model::ErrorCode;
use reqwest::RequestBuilder;
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{
    BUSY_NOTICE, Disposition, Incoming, admit_waiting, chunks, deliver, expose, json_response,
    reply_parts, supervise,
};
use crate::service::Bots;

const API: &str = "https://ilinkai.weixin.qq.com";
const APP_ID: &str = "bot";
/// iLink's encoding of client version 2.2.0: `(major << 16) | (minor << 8) | patch`.
const CLIENT_VERSION: &str = "131584";
const CHANNEL_VERSION: &str = "2.2.0";
const UPDATES_PATH: &str = "ilink/bot/getupdates";
const SEND_PATH: &str = "ilink/bot/sendmessage";
const LONG_POLL: Duration = Duration::from_secs(35);
const MAX_TEXT_BYTES: usize = 16 * 1024;
const MAX_CHUNK_UTF16: usize = 2000;
const ITEM_TEXT: i64 = 1;
const MESSAGE_FROM_BOT: i64 = 2;
const MESSAGE_FINISHED: i64 = 2;
const SESSION_EXPIRED: i64 = -14;
const RATE_LIMITED: i64 = -2;
const SEND_ATTEMPTS: usize = 3;
const RATE_LIMIT_DELAY: Duration = Duration::from_secs(2);
const EXPIRED_RETRY: Duration = Duration::from_secs(600);
const MIN_RETRY: Duration = Duration::from_secs(1);
const MAX_RETRY: Duration = Duration::from_secs(60);
const EXPIRED_NOTICE: &str = "WeChat ended this session. Delete this channel and scan again.";

pub(super) async fn run(
    bots: &Bots,
    channel: &Channel,
    client: &reqwest::Client,
    token: &SecretString,
    cancel: &CancellationToken,
) -> Result<(), ErrorCode> {
    let ChannelConfig::Weixin { base_url, .. } = &channel.config else {
        return Err(ErrorCode::InvalidMessage);
    };
    run_at(bots, channel, (client, token, base_url), cancel).await
}

async fn run_at(
    bots: &Bots,
    channel: &Channel,
    transport: (&reqwest::Client, &SecretString, &str),
    cancel: &CancellationToken,
) -> Result<(), ErrorCode> {
    let ChannelConfig::Weixin { account_id, .. } = &channel.config else {
        return Err(ErrorCode::InvalidMessage);
    };
    let (client, token, base) = transport;
    bots.account(channel.id, format!("weixin:{account_id}"))
        .await?;
    bots.channel_status(
        channel.id,
        json!({"state":"connected","accountId":account_id}),
    )
    .await;
    let connection = Connection {
        bots,
        channel,
        client,
        token,
        base,
        account_id,
    };
    let connection = &connection;
    let stop = cancel.child_token();
    let sending = deliver(bots, channel.id, &stop, |turn, conversation| async move {
        connection.reply(turn, conversation).await
    });
    supervise(cancel, &stop, connection.receive(), sending).await
}

/// Whether `base` is an iLink API root that may receive this channel's token.
pub(crate) fn trusted_base(base: &str) -> bool {
    url::Url::parse(base).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some_and(trusted_host)
            && url.port().is_none()
            && url.username().is_empty()
            && url.password().is_none()
            && url.path() == "/"
            && url.query().is_none()
            && url.fragment().is_none()
    })
}

fn trusted_host(host: &str) -> bool {
    host == "weixin.qq.com" || host.ends_with(".weixin.qq.com")
}

/// Add the version headers every iLink request carries.
fn versioned(request: RequestBuilder) -> RequestBuilder {
    request
        .header("iLink-App-Id", APP_ID)
        .header("iLink-App-ClientVersion", CLIENT_VERSION)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reply {
    Accepted,
    Expired,
    RateLimited,
    Rejected,
}

fn session_expired(response: &Value) -> bool {
    response["ret"] == SESSION_EXPIRED || response["errcode"] == SESSION_EXPIRED
}

/// Parse a response, treating any HTTP failure as a failure whatever its body says.
async fn checked_json(response: reqwest::Response) -> Result<Value, ErrorCode> {
    if !response.status().is_success() {
        return Err(ErrorCode::BotIo);
    }
    json_response(response).await
}

fn classify(response: &Value) -> Reply {
    let ret = response["ret"].as_i64().unwrap_or(0);
    let errcode = response["errcode"].as_i64().unwrap_or(0);
    if ret == 0 && errcode == 0 {
        return Reply::Accepted;
    }
    if session_expired(response) {
        return Reply::Expired;
    }
    if ret == RATE_LIMITED || errcode == RATE_LIMITED {
        // iLink reuses its rate-limit code for a conversation it can no longer continue.
        let message = response["errmsg"]
            .as_str()
            .or_else(|| response["msg"].as_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        return if matches!(message.as_str(), "unknown error" | "prepare failed") {
            Reply::Expired
        } else {
            Reply::RateLimited
        };
    }
    Reply::Rejected
}

struct Connection<'a> {
    bots: &'a Bots,
    channel: &'a Channel,
    client: &'a reqwest::Client,
    token: &'a SecretString,
    base: &'a str,
    account_id: &'a str,
}

impl Connection<'_> {
    fn request(&self, path: &str, body: &Value) -> RequestBuilder {
        let mut body = body.clone();
        body["base_info"] = json!({"channel_version":CHANNEL_VERSION});
        let uin = STANDARD.encode(Uuid::new_v4().as_fields().0.to_string());
        versioned(self.client.post(format!("{}/{path}", self.base)))
            .bearer_auth(expose(self.token))
            .header("AuthorizationType", "ilink_bot_token")
            .header("X-WECHAT-UIN", uin)
            .json(&body)
    }

    /// Wait for updates; a poll that outlasts its timeout simply found nothing new.
    async fn updates(&self, buffer: &str) -> Result<Value, ErrorCode> {
        let request = self
            .request(UPDATES_PATH, &json!({"get_updates_buf":buffer}))
            .timeout(LONG_POLL);
        match request.send().await {
            Ok(response) => checked_json(response).await,
            Err(error) if error.is_timeout() => Ok(json!({})),
            Err(_) => Err(ErrorCode::BotIo),
        }
    }

    async fn receive(&self) -> Result<(), ErrorCode> {
        let mut buffer = self.channel.sync_token.clone().unwrap_or_default();
        let mut interrupted = false;
        let mut retry = MIN_RETRY;
        loop {
            // Failures retry here rather than reconnecting, which would rewind `buffer` to the
            // position this connection started from.
            let polled = self.updates(&buffer).await;
            let response = match &polled {
                Ok(response) if classify(response) == Reply::Accepted => response,
                failed => {
                    let expired = failed.as_ref().is_ok_and(session_expired);
                    let (status, delay) = if expired {
                        (
                            json!({"state":"expired","error":EXPIRED_NOTICE}),
                            EXPIRED_RETRY,
                        )
                    } else {
                        (json!({"state":"reconnecting"}), retry)
                    };
                    self.bots.channel_status(self.channel.id, status).await;
                    interrupted = true;
                    retry = (retry * 2).min(MAX_RETRY);
                    tokio::time::sleep(delay).await;
                    continue;
                }
            };
            retry = MIN_RETRY;
            if interrupted {
                interrupted = false;
                self.bots
                    .channel_status(
                        self.channel.id,
                        json!({"state":"connected","accountId":self.account_id}),
                    )
                    .await;
            }
            for message in response["msgs"].as_array().into_iter().flatten() {
                if let Some(incoming) = incoming(self.channel, message, self.account_id)? {
                    self.admit(incoming).await?;
                }
            }
            // Only after the batch is queued, so a crash replays it rather than losing it.
            if let Some(next) = response["get_updates_buf"]
                .as_str()
                .filter(|next| !next.is_empty() && *next != buffer)
            {
                self.bots
                    .sync_token(self.channel.id, next.to_owned())
                    .await?;
                next.clone_into(&mut buffer);
            }
        }
    }

    async fn admit(&self, incoming: Incoming) -> Result<(), ErrorCode> {
        if admit_waiting(self.bots, incoming.clone()).await? == Disposition::Busy {
            let client_id = format!("busy-{}", incoming.message_id);
            let notice = self.post_text(
                &incoming.peer,
                incoming.reply_to.as_deref(),
                BUSY_NOTICE,
                &client_id,
            );
            if let Err(error) = notice.await {
                tracing::warn!(channel = %self.channel.id, ?error, "WeChat busy notice failed");
            }
        }
        Ok(())
    }

    async fn reply(&self, turn: Turn, conversation: Conversation) -> Result<(), ErrorCode> {
        let parts = reply_parts(&turn);
        if parts.is_empty() {
            return Err(ErrorCode::BotIo);
        }
        let mut index = 0;
        for text in parts {
            for chunk in chunks(text, MAX_CHUNK_UTF16) {
                self.post_text(
                    &conversation.peer,
                    turn.reply_to.as_deref(),
                    chunk,
                    &format!("ait-{}-{index}", turn.id),
                )
                .await?;
                index += 1;
            }
        }
        Ok(())
    }

    /// Send one text message; `client_id` lets iLink recognize a retried send.
    async fn post_text(
        &self,
        peer: &str,
        context: Option<&str>,
        text: &str,
        client_id: &str,
    ) -> Result<(), ErrorCode> {
        let mut context = context;
        for _ in 0..SEND_ATTEMPTS {
            let mut message = json!({
                "from_user_id":"",
                "to_user_id":peer,
                "client_id":client_id,
                "message_type":MESSAGE_FROM_BOT,
                "message_state":MESSAGE_FINISHED,
                "item_list":[{"type":ITEM_TEXT,"text_item":{"text":text}}],
            });
            if let Some(token) = context {
                message["context_token"] = json!(token);
            }
            let response = checked_json(
                self.request(SEND_PATH, &json!({"msg":message}))
                    .send()
                    .await
                    .map_err(|_| ErrorCode::BotIo)?,
            )
            .await?;
            match classify(&response) {
                Reply::Accepted => return Ok(()),
                // iLink accepts a send without a conversation token it has already retired.
                Reply::Expired if context.is_some() => context = None,
                Reply::RateLimited => tokio::time::sleep(RATE_LIMIT_DELAY).await,
                Reply::Expired | Reply::Rejected => return Err(ErrorCode::BotIo),
            }
        }
        Err(ErrorCode::BotIo)
    }
}

fn identifier(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn is_group(message: &Value, account_id: &str) -> bool {
    let room = |key: &str| message[key].as_str().is_some_and(|room| !room.is_empty());
    if room("room_id") || room("chat_room_id") || room("group_id") {
        return true;
    }
    message["msg_type"] == 1
        && message["to_user_id"]
            .as_str()
            .is_some_and(|target| !target.is_empty() && target != account_id)
}

fn message_text(message: &Value) -> Option<&str> {
    message["item_list"]
        .as_array()?
        .iter()
        .find(|item| item["type"] == ITEM_TEXT)?["text_item"]["text"]
        .as_str()
        .filter(|text| !text.trim().is_empty() && text.len() <= MAX_TEXT_BYTES)
}

fn incoming(
    channel: &Channel,
    message: &Value,
    account_id: &str,
) -> Result<Option<Incoming>, ErrorCode> {
    let ChannelConfig::Weixin {
        allowed_senders, ..
    } = &channel.config
    else {
        return Err(ErrorCode::InvalidMessage);
    };
    let Some(sender) = message["from_user_id"].as_str() else {
        return Ok(None);
    };
    if message["message_type"] == MESSAGE_FROM_BOT
        || sender == account_id
        || !allowed_senders.iter().any(|allowed| allowed == sender)
        || is_group(message, account_id)
    {
        return Ok(None);
    }
    let Some(text) = message_text(message) else {
        return Ok(None);
    };
    let Some(message_id) = identifier(&message["message_id"])
        .or_else(|| identifier(&message["seq"]).map(|seq| format!("{sender}:{seq}")))
    else {
        return Ok(None);
    };
    Ok(Some(Incoming {
        channel_id: channel.id,
        peer: sender.to_owned(),
        sender: sender.to_owned(),
        thread: None,
        message_id,
        text: text.to_owned(),
        reply_to: message["context_token"]
            .as_str()
            .filter(|token| !token.is_empty())
            .map(str::to_owned),
    }))
}

#[cfg(test)]
mod tests;
