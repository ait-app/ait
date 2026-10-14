mod frames;

use std::sync::Arc;
use std::time::{Duration, Instant};

use domain::bot::{Channel, ChannelConfig};
use futures_util::{SinkExt, StreamExt};
use model::ErrorCode;
use prost::Message;
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::{Message as WsMessage, protocol::WebSocketConfig};
use tokio_util::sync::CancellationToken;

use super::{Incoming, chunks, deliver, expose, json_response};
use crate::service::Bots;
use frames::{Fragments, Frame, Header};

const API: &str = "https://open.feishu.cn";

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
}

pub(super) async fn run(
    bots: &Bots,
    channel: &Channel,
    client: &reqwest::Client,
    secret: &SecretString,
    cancel: &CancellationToken,
) -> Result<(), ErrorCode> {
    run_at(bots, channel, (client, secret, API), cancel, connect).await
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
    let ChannelConfig::Feishu { app_id, .. } = &channel.config else {
        return Err(ErrorCode::InvalidMessage);
    };
    let connection = Arc::new(Connection {
        channel,
        client,
        app_id,
        secret,
        token: Mutex::new(None),
        api,
    });
    let bot_info = connection
        .call("GET", "/open-apis/bot/v3/info", None)
        .await?;
    let bot_id = bot_info["bot"]["open_id"]
        .as_str()
        .ok_or(ErrorCode::BotIo)?
        .to_owned();
    bots.account(channel.id, format!("feishu:{app_id}")).await?;
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
            .is_none_or(|host| !host.ends_with(".feishu.cn"))
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

async fn connect(endpoint: String) -> Result<Socket, ErrorCode> {
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

struct StreamOptions<'a> {
    service: i32,
    ping_interval: u64,
    bot_id: &'a str,
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

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
                let accepted = match incoming(connection.channel, &event, options.bot_id)? {
                    Some(incoming) => bots.accept(incoming).await.map(|_| ()),
                    None => Ok(()),
                };
                frame.payload = Some(serde_json::to_vec(&json!({"code":if accepted.is_ok() {200} else {500},"headers":null,"data":null})).map_err(|_| ErrorCode::BotIo)?);
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
    let ChannelConfig::Feishu {
        allowed_senders,
        allow_groups,
        ..
    } = &channel.config
    else {
        return Err(ErrorCode::InvalidMessage);
    };
    if event["header"]["event_type"] != "im.message.receive_v1" {
        return Ok(None);
    }
    let sender = &event["event"]["sender"];
    let Some(sender_id) = sender["sender_id"]["open_id"].as_str() else {
        return Ok(None);
    };
    if sender["sender_type"] != "user"
        || !allowed_senders.iter().any(|allowed| allowed == sender_id)
    {
        return Ok(None);
    }
    let message = &event["event"]["message"];
    if message["message_type"] != "text" {
        return Ok(None);
    }
    if message["chat_type"] != "p2p"
        && (!allow_groups
            || message["mentions"].as_array().is_none_or(|mentions| {
                !mentions
                    .iter()
                    .any(|mention| mention["id"]["open_id"] == bot_id)
            }))
    {
        return Ok(None);
    }
    let content: Value = serde_json::from_str(message["content"].as_str().ok_or(ErrorCode::BotIo)?)
        .map_err(|_| ErrorCode::BotIo)?;
    let Some(text) = content["text"]
        .as_str()
        .filter(|text| !text.trim().is_empty() && text.len() <= 16 * 1024)
    else {
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
        message_id: event["header"]["event_id"]
            .as_str()
            .ok_or(ErrorCode::BotIo)?
            .to_owned(),
        text: text.to_owned(),
        reply_to: message["message_id"].as_str().map(str::to_owned),
    }))
}

impl Connection<'_> {
    async fn reply(
        &self,
        turn: domain::bot::Turn,
        conversation: domain::bot::Conversation,
    ) -> Result<(), ErrorCode> {
        let text = turn.reply.as_deref().ok_or(ErrorCode::BotIo)?;
        for (index, chunk) in chunks(text, 3000).into_iter().enumerate() {
            let content =
                serde_json::to_string(&json!({"text":chunk})).map_err(|_| ErrorCode::BotIo)?;
            let body = json!({"receive_id":conversation.peer,"msg_type":"text","content":content,
                "uuid":format!("{}-{index}",turn.id)});
            if let Some(message) = &turn.reply_to {
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
                body["reply_in_thread"] = json!(conversation.thread.is_some());
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
        }
        Ok(())
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
