mod content;
mod frames;
pub(crate) mod onboarding;

use std::sync::Arc;
use std::time::{Duration, Instant};

use domain::bot::{Channel, ChannelConfig};
use futures_util::{SinkExt, StreamExt};
use model::ErrorCode;
use prost::Message;
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_util::sync::CancellationToken;

use super::{
    BUSY_NOTICE, Disposition, Incoming, Socket, admit, chunks, connect_socket, deliver, expose,
    json_response, media,
};
use crate::service::Bots;
use frames::{Fragments, Frame, Header};

const API: &str = "https://open.feishu.cn";
const LARK_API: &str = "https://open.larksuite.com";

/// Region selected by the channel kind and its credential-verification RPC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Platform {
    /// China-region Feishu application.
    Feishu,
    /// International Lark application.
    Lark,
}

impl Platform {
    fn api(self) -> &'static str {
        match self {
            Self::Feishu => API,
            Self::Lark => LARK_API,
        }
    }
}

struct Token {
    secret: SecretString,
    expires: Instant,
}

struct Connection<'a> {
    channel: &'a Channel,
    client: &'a reqwest::Client,
    app_id: &'a str,
    secret: &'a SecretString,
    token: Mutex<Option<Token>>,
    api: &'a str,
    image_directory: Option<std::path::PathBuf>,
}

pub(super) async fn run(
    bots: &Bots,
    channel: &Channel,
    client: &reqwest::Client,
    secret: &SecretString,
    cancel: &CancellationToken,
) -> Result<(), ErrorCode> {
    let platform = match &channel.config {
        ChannelConfig::Feishu { .. } => Platform::Feishu,
        ChannelConfig::Lark { .. } => Platform::Lark,
        ChannelConfig::Builtin
        | ChannelConfig::Webhook { .. }
        | ChannelConfig::Telegram { .. }
        | ChannelConfig::Weixin { .. }
        | ChannelConfig::Qq { .. }
        | ChannelConfig::PhotonImessage { .. } => return Err(ErrorCode::InvalidMessage),
    };
    run_at(
        bots,
        channel,
        (client, secret, platform.api()),
        cancel,
        connect_socket,
    )
    .await
}

async fn run_at<F, Fut>(
    bots: &Bots,
    channel: &Channel,
    transport: (&reqwest::Client, &SecretString, &str),
    cancel: &CancellationToken,
    dial: F,
) -> Result<(), ErrorCode>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<Socket, ErrorCode>>,
{
    let (client, secret, api) = transport;
    let (app_id, account_kind, socket_suffix) = match &channel.config {
        ChannelConfig::Feishu { app_id, .. } => (app_id, "feishu", ".feishu.cn"),
        ChannelConfig::Lark { app_id, .. } => (app_id, "lark", ".larksuite.com"),
        ChannelConfig::Builtin
        | ChannelConfig::Webhook { .. }
        | ChannelConfig::Telegram { .. }
        | ChannelConfig::Weixin { .. }
        | ChannelConfig::Qq { .. }
        | ChannelConfig::PhotonImessage { .. } => return Err(ErrorCode::InvalidMessage),
    };
    let connection = Arc::new(Connection {
        channel,
        client,
        app_id,
        secret,
        token: Mutex::new(None),
        api,
        image_directory: bots.image_directory(),
    });
    let bot_info = connection
        .call("GET", "/open-apis/bot/v3/info", None)
        .await?;
    let bot_id = bot_info["bot"]["open_id"]
        .as_str()
        .ok_or(ErrorCode::BotIo)?
        .to_owned();
    bots.account(channel.id, format!("{account_kind}:{app_id}"))
        .await?;
    let endpoint = json_response(
        client
            .post(format!("{api}/callback/ws/endpoint"))
            .json(&json!({"AppID":app_id,"AppSecret":expose(secret)}))
            .send()
            .await
            .map_err(|_| ErrorCode::BotIo)?,
    )
    .await?;
    if endpoint["code"]
        .as_i64()
        .or_else(|| endpoint["Code"].as_i64())
        != Some(0)
    {
        return Err(ErrorCode::BotIo);
    }
    let data = endpoint
        .get("data")
        .or_else(|| endpoint.get("Data"))
        .ok_or(ErrorCode::BotIo)?;
    let endpoint = data["URL"].as_str().ok_or(ErrorCode::BotIo)?;
    let url = url::Url::parse(endpoint).map_err(|_| ErrorCode::BotIo)?;
    if url.scheme() != "wss"
        || url
            .host_str()
            .is_none_or(|host| !host.ends_with(socket_suffix))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(ErrorCode::BotIo);
    }
    let service = url
        .query_pairs()
        .find(|(key, _)| key == "service_id")
        .and_then(|(_, value)| value.parse::<i32>().ok())
        .ok_or(ErrorCode::BotIo)?;
    let ping_interval = data["ClientConfig"]["PingInterval"]
        .as_u64()
        .unwrap_or(120)
        .clamp(10, 300);
    let socket = dial(endpoint.to_owned()).await?;
    bots.channel_status(channel.id, json!({"state":"connected","accountId":app_id}))
        .await;
    let receiving = receive(
        bots,
        &connection,
        socket,
        StreamOptions {
            service,
            ping_interval,
            bot_id: &bot_id,
        },
        cancel,
    );
    let sending = deliver(bots, channel.id, cancel, |turn, conversation| {
        let connection = connection.clone();
        async move { connection.reply(turn, conversation).await }
    });
    tokio::select! {
        () = cancel.cancelled() => Ok(()),
        result = receiving => result,
        result = sending => result,
    }
}

struct StreamOptions<'a> {
    service: i32,
    ping_interval: u64,
    bot_id: &'a str,
}

async fn receive(
    bots: &Bots,
    connection: &Connection<'_>,
    mut socket: Socket,
    options: StreamOptions<'_>,
    cancel: &CancellationToken,
) -> Result<(), ErrorCode> {
    let mut fragments = Fragments::default();
    let mut heartbeat = tokio::time::interval(Duration::from_secs(options.ping_interval));
    let mut last_received = Instant::now();
    loop {
        let message = tokio::select! {
            biased;
            () = cancel.cancelled() => { let _ = socket.close(None).await; return Ok(()); },
            _ = heartbeat.tick() => {
                if last_received.elapsed() > Duration::from_secs(options.ping_interval * 2 + 10) { return Err(ErrorCode::BotIo); }
                let frame = Frame { service: options.service, headers: vec![Header { key: "type".into(), value: "ping".into() }], ..Frame::default() };
                socket.send(WsMessage::Binary(frame.encode_to_vec().into())).await.map_err(|_| ErrorCode::BotIo)?;
                continue;
            },
            message = socket.next() => message.ok_or(ErrorCode::BotIo)?.map_err(|_| ErrorCode::BotIo)?,
        };
        last_received = Instant::now();
        match message {
            WsMessage::Binary(bytes) => {
                let mut frame = Frame::decode(bytes).map_err(|_| ErrorCode::BotIo)?;
                if frame.method != 1 || frame.header("type") != Some("event") {
                    continue;
                }
                let Some(payload) = fragments.combine(&frame)? else {
                    continue;
                };
                let event: Value =
                    serde_json::from_slice(&payload).map_err(|_| ErrorCode::BotIo)?;
                let acknowledged = match incoming(connection.channel, &event, options.bot_id)? {
                    Some(incoming)
                        if !bots
                            .admit_chat(
                                connection.channel.id,
                                incoming.peer.clone(),
                                event["event"]["message"]["chat_type"] != "p2p",
                                !incoming.passive,
                            )
                            .await? =>
                    {
                        true
                    }
                    Some(incoming) => connection.admit(bots, incoming).await,
                    None => true,
                };
                // A non-200 response makes the platform deliver the event again later.
                frame.payload = Some(serde_json::to_vec(&json!({"code":if acknowledged {200} else {500},"headers":null,"data":null})).map_err(|_| ErrorCode::BotIo)?);
                socket
                    .send(WsMessage::Binary(frame.encode_to_vec().into()))
                    .await
                    .map_err(|_| ErrorCode::BotIo)?;
            }
            WsMessage::Ping(bytes) => socket
                .send(WsMessage::Pong(bytes))
                .await
                .map_err(|_| ErrorCode::BotIo)?,
            WsMessage::Close(_) => return Err(ErrorCode::BotIo),
            WsMessage::Text(_) | WsMessage::Pong(_) | WsMessage::Frame(_) => {}
        }
    }
}

fn incoming(channel: &Channel, event: &Value, bot_id: &str) -> Result<Option<Incoming>, ErrorCode> {
    let (ChannelConfig::Feishu { .. } | ChannelConfig::Lark { .. }) = &channel.config else {
        return Err(ErrorCode::InvalidMessage);
    };
    if event["header"]["event_type"] != "im.message.receive_v1" {
        return Ok(None);
    }
    let sender = &event["event"]["sender"];
    let Some(sender_id) = sender["sender_id"]["open_id"].as_str() else {
        return Ok(None);
    };
    if !matches!(sender["sender_type"].as_str(), Some("user" | "bot")) || sender_id == bot_id {
        return Ok(None);
    }
    let message = &event["event"]["message"];
    let group = message["chat_type"] != "p2p";
    let mentioned = message["mentions"].as_array().is_some_and(|mentions| {
        mentions
            .iter()
            .any(|mention| mention["id"]["open_id"] == bot_id)
    });
    let content: Value = serde_json::from_str(message["content"].as_str().ok_or(ErrorCode::BotIo)?)
        .map_err(|_| ErrorCode::BotIo)?;
    let Some(text) = content::text(message["message_type"].as_str().unwrap_or(""), &content) else {
        return Ok(None);
    };
    Ok(Some(Incoming {
        channel_id: channel.id,
        peer: message["chat_id"]
            .as_str()
            .ok_or(ErrorCode::BotIo)?
            .to_owned(),
        sender: sender_id.to_owned(),
        thread: message["thread_id"]
            .as_str()
            .or_else(|| message["root_id"].as_str())
            .filter(|id| !id.is_empty())
            .map(str::to_owned),
        message_id: message["message_id"]
            .as_str()
            .ok_or(ErrorCode::BotIo)?
            .to_owned(),
        text,
        reply_to: message["message_id"].as_str().map(str::to_owned),
        passive: group && !mentioned,
        sender_type: sender["sender_type"].as_str().map(str::to_owned),
    }))
}

impl Connection<'_> {
    async fn reply(
        &self,
        turn: domain::bot::Turn,
        conversation: domain::bot::Conversation,
    ) -> Result<(), ErrorCode> {
        let parts = media::reply_parts(&turn);
        if parts.is_empty() {
            return Err(ErrorCode::BotIo);
        }
        let mut index = 0;
        for part in parts {
            match part {
                media::ReplyPart::Text(text) => {
                    for chunk in chunks(&text, 3000) {
                        self.post_text(
                            &conversation.peer,
                            turn.reply_to.as_deref(),
                            conversation.thread.is_some(),
                            chunk,
                            &format!("{}-{index}", turn.id),
                        )
                        .await?;
                        index += 1;
                    }
                }
                media::ReplyPart::Image { source, .. } => {
                    let image =
                        media::load_image(self.client, self.image_directory.as_deref(), &source)
                            .await?;
                    self.post_image(
                        &conversation.peer,
                        turn.reply_to.as_deref(),
                        conversation.thread.is_some(),
                        image,
                        &format!("{}-{index}", turn.id),
                    )
                    .await?;
                    index += 1;
                }
            }
        }
        Ok(())
    }

    async fn post_image(
        &self,
        peer: &str,
        reply_to: Option<&str>,
        in_thread: bool,
        image: media::ImageData,
        uuid: &str,
    ) -> Result<(), ErrorCode> {
        if let Some(message) = reply_to
            && !message
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(ErrorCode::BotIo);
        }
        let token = self.token().await?;
        let part = reqwest::multipart::Part::bytes(image.bytes)
            .file_name(image.file_name)
            .mime_str(image.mime_type)
            .map_err(|_| ErrorCode::BotIo)?;
        let form = reqwest::multipart::Form::new()
            .text("image_type", "message")
            .part("image", part);
        let upload = json_response(
            self.client
                .post(format!("{}/open-apis/im/v1/images", self.api))
                .bearer_auth(expose(&token))
                .multipart(form)
                .send()
                .await
                .map_err(|_| ErrorCode::BotIo)?,
        )
        .await?;
        if upload["code"] != 0 {
            return Err(ErrorCode::BotIo);
        }
        let image_key = upload["data"]["image_key"]
            .as_str()
            .filter(|key| !key.is_empty())
            .ok_or(ErrorCode::BotIo)?;
        let mut body = json!({
            "receive_id":peer,
            "msg_type":"image",
            "content":serde_json::to_string(&json!({"image_key":image_key})).map_err(|_| ErrorCode::BotIo)?,
            "uuid":uuid,
        });
        if let Some(message) = reply_to {
            body.as_object_mut()
                .ok_or(ErrorCode::BotIo)?
                .remove("receive_id");
            body["reply_in_thread"] = json!(in_thread);
            self.call(
                "POST",
                &format!("/open-apis/im/v1/messages/{message}/reply"),
                Some(body),
            )
            .await?;
        } else {
            self.call(
                "POST",
                "/open-apis/im/v1/messages?receive_id_type=chat_id",
                Some(body),
            )
            .await?;
        }
        Ok(())
    }

    /// Send one text message, as a reply to `reply_to` when present; `uuid` deduplicates retries.
    async fn post_text(
        &self,
        peer: &str,
        reply_to: Option<&str>,
        in_thread: bool,
        text: &str,
        uuid: &str,
    ) -> Result<(), ErrorCode> {
        let content = serde_json::to_string(&json!({"text":text})).map_err(|_| ErrorCode::BotIo)?;
        let body = json!({"receive_id":peer,"msg_type":"text","content":content,"uuid":uuid});
        if let Some(message) = reply_to {
            if !message
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                return Err(ErrorCode::BotIo);
            }
            let mut body = body;
            body.as_object_mut()
                .ok_or(ErrorCode::BotIo)?
                .remove("receive_id");
            body["reply_in_thread"] = json!(in_thread);
            self.call(
                "POST",
                &format!("/open-apis/im/v1/messages/{message}/reply"),
                Some(body),
            )
            .await?;
        } else {
            self.call(
                "POST",
                "/open-apis/im/v1/messages?receive_id_type=chat_id",
                Some(body),
            )
            .await?;
        }
        Ok(())
    }

    /// Admit one event and report whether Feishu may consider it delivered.
    /// A full Bot backlog and internal failures leave it unacknowledged for redelivery; a
    /// saturated conversation drops only its own message and tells the sender.
    async fn admit(&self, bots: &Bots, incoming: Incoming) -> bool {
        match admit(bots, incoming.clone()).await {
            Ok(Disposition::Accepted | Disposition::Skip) => true,
            Ok(Disposition::Retry) | Err(_) => false,
            Ok(Disposition::Busy) => {
                let uuid = format!("busy-{}", incoming.message_id);
                let notice = self.post_text(
                    &incoming.peer,
                    incoming.reply_to.as_deref(),
                    incoming.thread.is_some(),
                    BUSY_NOTICE,
                    &uuid,
                );
                if let Err(error) = notice.await {
                    tracing::warn!(channel = %self.channel.id, ?error, "Feishu busy notice failed");
                }
                true
            }
        }
    }

    async fn token(&self) -> Result<SecretString, ErrorCode> {
        let mut cached = self.token.lock().await;
        if let Some(token) = &*cached
            && token.expires > Instant::now()
        {
            return Ok(token.secret.clone());
        }
        let value = json_response(
            self.client
                .post(format!(
                    "{}/open-apis/auth/v3/tenant_access_token/internal",
                    self.api
                ))
                .json(&json!({"app_id":self.app_id,"app_secret":expose(self.secret)}))
                .send()
                .await
                .map_err(|_| ErrorCode::BotIo)?,
        )
        .await?;
        if value["code"] != 0 {
            return Err(ErrorCode::BotIo);
        }
        let secret = SecretString::from(
            value["tenant_access_token"]
                .as_str()
                .ok_or(ErrorCode::BotIo)?
                .to_owned(),
        );
        let seconds = value["expire"]
            .as_u64()
            .ok_or(ErrorCode::BotIo)?
            .saturating_sub(60)
            .min(7200);
        *cached = Some(Token {
            secret: secret.clone(),
            expires: Instant::now() + Duration::from_secs(seconds),
        });
        Ok(secret)
    }
    async fn call(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, ErrorCode> {
        let token = self.token().await?;
        let request = self
            .client
            .request(
                if method == "POST" {
                    reqwest::Method::POST
                } else {
                    reqwest::Method::GET
                },
                format!("{}{path}", self.api),
            )
            .bearer_auth(expose(&token));
        let request = if let Some(body) = body {
            request.json(&body)
        } else {
            request
        };
        let value = json_response(request.send().await.map_err(|_| ErrorCode::BotIo)?).await?;
        if value["code"] != 0 {
            return Err(ErrorCode::BotIo);
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests;
