//! QQ channel: the official bot gateway for private chats and group @mentions; see ADR-225.

pub(crate) mod onboarding;

use std::time::{Duration, Instant};

use domain::bot::{Channel, ChannelConfig, Conversation, Turn};
use futures_util::{SinkExt, StreamExt};
use model::ErrorCode;
use reqwest::header::AUTHORIZATION;
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_util::sync::CancellationToken;

use super::{
    BUSY_NOTICE, Disposition, Incoming, Socket, admit_waiting, chunks, connect_socket, deliver,
    expose, json_response, reply_parts, supervise,
};
use crate::service::Bots;

const API: &str = "https://api.sgroup.qq.com";
const TOKEN_URL: &str = "https://bots.qq.com/app/getAppAccessToken";
/// `GROUP_AND_C2C_EVENT`: private messages and group messages.
const INTENTS: u64 = 1 << 25;
const PRIVATE_MESSAGE: &str = "C2C_MESSAGE_CREATE";
const GROUP_AT_MESSAGE: &str = "GROUP_AT_MESSAGE_CREATE";
/// Full group-message delivery can replace the mention-only event when QQ enables it.
const GROUP_ALL_MESSAGE: &str = "GROUP_MESSAGE_CREATE";
const PRIVATE_PEER: &str = "c2c";
const GROUP_PEER: &str = "group";
const OP_DISPATCH: u64 = 0;
const OP_HEARTBEAT: u64 = 1;
const OP_IDENTIFY: u64 = 2;
const OP_RECONNECT: u64 = 7;
const OP_INVALID_SESSION: u64 = 9;
const OP_HELLO: u64 = 10;
const HELLO_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_TEXT_BYTES: usize = 16 * 1024;
const MAX_CHUNK_UTF16: usize = 4000;
/// Replies QQ allows per inbound message, using the lower of its private and group limits.
const PASSIVE_REPLIES: usize = 4;
/// QQ's refusals for a reply outside its time or count limit, and for an expired `msg_id`.
const REPLY_WINDOW_CLOSED: [i64; 2] = [40_034_128, 40_034_005];
const PAIRED_NOTICE: &str = "Paired. This bot now accepts your messages here.";

/// The two QQ hosts a connection talks to, separated so tests can replace them.
#[derive(Clone, Copy)]
struct Hosts<'a> {
    token: &'a str,
    api: &'a str,
}

struct Token {
    secret: SecretString,
    expires: Instant,
}

struct Connection<'a> {
    bots: &'a Bots,
    channel: &'a Channel,
    client: &'a reqwest::Client,
    app_id: &'a str,
    secret: &'a SecretString,
    hosts: Hosts<'a>,
    token: Mutex<Option<Token>>,
}

pub(super) async fn run(
    bots: &Bots,
    channel: &Channel,
    client: &reqwest::Client,
    secret: &SecretString,
    cancel: &CancellationToken,
) -> Result<(), ErrorCode> {
    let hosts = Hosts {
        token: TOKEN_URL,
        api: API,
    };
    run_at(
        bots,
        channel,
        (client, secret, hosts),
        cancel,
        connect_socket,
    )
    .await
}

async fn run_at<F, Fut>(
    bots: &Bots,
    channel: &Channel,
    transport: (&reqwest::Client, &SecretString, Hosts<'_>),
    cancel: &CancellationToken,
    dial: F,
) -> Result<(), ErrorCode>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<Socket, ErrorCode>>,
{
    let ChannelConfig::Qq { app_id, .. } = &channel.config else {
        return Err(ErrorCode::InvalidMessage);
    };
    let (client, secret, hosts) = transport;
    bots.account(channel.id, format!("qq:{app_id}")).await?;
    let connection = Connection {
        bots,
        channel,
        client,
        app_id,
        secret,
        hosts,
        token: Mutex::new(None),
    };
    let gateway = connection
        .call(reqwest::Method::GET, "/gateway", None)
        .await?;
    let endpoint = trusted_gateway(gateway["url"].as_str().ok_or(ErrorCode::BotIo)?)?;
    let socket = dial(endpoint).await?;
    let connection = &connection;
    let stop = cancel.child_token();
    let sending = deliver(bots, channel.id, &stop, |turn, conversation| async move {
        connection.reply(turn, conversation).await
    });
    // The socket makes this future large enough to be worth keeping off the stack.
    let receiving = Box::pin(connection.receive(socket));
    supervise(cancel, &stop, receiving, sending).await
}

fn trusted_gateway(endpoint: &str) -> Result<String, ErrorCode> {
    let url = url::Url::parse(endpoint).map_err(|_| ErrorCode::BotIo)?;
    let trusted = url
        .host_str()
        .is_some_and(|host| host == "qq.com" || host.ends_with(".qq.com"));
    if url.scheme() != "wss" || !trusted {
        return Err(ErrorCode::BotIo);
    }
    Ok(url.into())
}

/// Exchange an App ID and secret for an access token and its lifetime in seconds.
async fn fetch_token(
    client: &reqwest::Client,
    url: &str,
    app_id: &str,
    secret: &SecretString,
) -> Result<(SecretString, u64), ErrorCode> {
    let value = json_response(
        client
            .post(url)
            .json(&json!({"appId":app_id,"clientSecret":expose(secret)}))
            .send()
            .await
            .map_err(|_| ErrorCode::BotIo)?,
    )
    .await?;
    let token = value["access_token"]
        .as_str()
        .filter(|token| !token.is_empty())
        .ok_or(ErrorCode::BotIo)?;
    // QQ returns the lifetime as a string.
    let lifetime = value["expires_in"]
        .as_u64()
        .or_else(|| value["expires_in"].as_str()?.parse().ok())
        .ok_or(ErrorCode::BotIo)?;
    Ok((SecretString::from(token.to_owned()), lifetime))
}

/// One decoded gateway frame; anything else on the socket needs no handling.
fn decode_frame(message: WsMessage) -> Result<Option<Value>, ErrorCode> {
    match message {
        WsMessage::Text(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|_| ErrorCode::BotIo),
        WsMessage::Close(_) => Err(ErrorCode::BotIo),
        WsMessage::Binary(_) | WsMessage::Ping(_) | WsMessage::Pong(_) | WsMessage::Frame(_) => {
            Ok(None)
        }
    }
}

async fn next_frame(socket: &mut Socket) -> Result<Option<Value>, ErrorCode> {
    let message = socket
        .next()
        .await
        .ok_or(ErrorCode::BotIo)?
        .map_err(|_| ErrorCode::BotIo)?;
    decode_frame(message)
}

async fn send_frame(socket: &mut Socket, frame: &Value) -> Result<(), ErrorCode> {
    socket
        .send(WsMessage::Text(frame.to_string().into()))
        .await
        .map_err(|_| ErrorCode::BotIo)
}

/// Finish `work` while still answering QQ's heartbeat schedule, because admission can wait out
/// a full backlog and a missed heartbeat would drop messages QQ never redelivers.
async fn keep_alive<T>(
    socket: &mut Socket,
    heartbeat: &mut tokio::time::Interval,
    sequence: Option<i64>,
    work: impl Future<Output = T>,
) -> Result<T, ErrorCode> {
    tokio::pin!(work);
    loop {
        tokio::select! {
            result = &mut work => return Ok(result),
            _ = heartbeat.tick() => {
                send_frame(socket, &json!({"op":OP_HEARTBEAT,"d":sequence})).await?;
            }
        }
    }
}

/// Wait for Hello and return how often QQ expects a heartbeat.
async fn await_hello(socket: &mut Socket) -> Result<Duration, ErrorCode> {
    let hello = async {
        loop {
            if let Some(frame) = next_frame(socket).await?
                && frame["op"] == OP_HELLO
            {
                let millis = frame["d"]["heartbeat_interval"]
                    .as_u64()
                    .ok_or(ErrorCode::BotIo)?;
                return Ok::<_, ErrorCode>(Duration::from_millis(millis));
            }
        }
    };
    tokio::time::timeout(HELLO_TIMEOUT, hello)
        .await
        .map_err(|_| ErrorCode::BotIo)?
}

impl Connection<'_> {
    async fn access_token(&self) -> Result<SecretString, ErrorCode> {
        let mut cached = self.token.lock().await;
        if let Some(token) = &*cached
            && token.expires > Instant::now()
        {
            return Ok(token.secret.clone());
        }
        let (secret, lifetime) =
            fetch_token(self.client, self.hosts.token, self.app_id, self.secret).await?;
        *cached = Some(Token {
            secret: secret.clone(),
            expires: Instant::now() + Duration::from_secs(lifetime.saturating_sub(60)),
        });
        Ok(secret)
    }

    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, ErrorCode> {
        let token = self.access_token().await?;
        let request = self
            .client
            .request(method, format!("{}{path}", self.hosts.api))
            .header(AUTHORIZATION, format!("QQBot {}", expose(&token)));
        let request = match body {
            Some(body) => request.json(body),
            None => request,
        };
        let response = request.send().await.map_err(|_| ErrorCode::BotIo)?;
        let succeeded = response.status().is_success();
        let value = json_response(response).await?;
        // A failure that carries QQ's error code is left for the caller to interpret.
        if !succeeded && refusal(&value).is_none() {
            return Err(ErrorCode::BotIo);
        }
        Ok(value)
    }

    async fn receive(&self, mut socket: Socket) -> Result<(), ErrorCode> {
        let period = await_hello(&mut socket).await?;
        let token = self.access_token().await?;
        let identify = json!({"op":OP_IDENTIFY,"d":{
            "token":format!("QQBot {}", expose(&token)),
            "intents":INTENTS,
            "shard":[0,1],
            "properties":{"$os":std::env::consts::OS,"$browser":"ait","$device":"ait"},
        }});
        send_frame(&mut socket, &identify).await?;
        let mut heartbeat = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
        let mut sequence: Option<i64> = None;
        let mut last_received = Instant::now();
        loop {
            let frame = tokio::select! {
                _ = heartbeat.tick() => {
                    if last_received.elapsed() > period * 2 + Duration::from_secs(10) {
                        return Err(ErrorCode::BotIo);
                    }
                    send_frame(&mut socket, &json!({"op":OP_HEARTBEAT,"d":sequence})).await?;
                    continue;
                }
                frame = next_frame(&mut socket) => frame?,
            };
            last_received = Instant::now();
            let Some(frame) = frame else {
                continue;
            };
            if let Some(next) = frame["s"].as_i64() {
                sequence = Some(next);
            }
            match frame["op"].as_u64() {
                Some(OP_DISPATCH) => {
                    let dispatch = self.dispatch(&frame);
                    keep_alive(&mut socket, &mut heartbeat, sequence, dispatch).await??;
                    last_received = Instant::now();
                }
                Some(OP_HEARTBEAT) => {
                    send_frame(&mut socket, &json!({"op":OP_HEARTBEAT,"d":sequence})).await?;
                }
                Some(OP_RECONNECT | OP_INVALID_SESSION) => return Err(ErrorCode::BotIo),
                _ => {}
            }
        }
    }

    async fn dispatch(&self, frame: &Value) -> Result<(), ErrorCode> {
        let event = frame["t"].as_str().unwrap_or_default();
        if event == "READY" {
            self.bots
                .channel_status(
                    self.channel.id,
                    json!({"state":"connected","accountId":self.app_id}),
                )
                .await;
            return Ok(());
        }
        let Some(incoming) = inbound(self.channel, event, &frame["d"])? else {
            return Ok(());
        };
        if allowed(self.channel, &incoming.sender) {
            return self.admit(incoming).await;
        }
        self.try_pair(&incoming).await
    }

    /// Add an unknown sender who presents the channel's one-time pairing code.
    /// The notice goes first because saving the sender restarts this connection.
    async fn try_pair(&self, incoming: &Incoming) -> Result<(), ErrorCode> {
        if !self.bots.claim_qq_pairing(self.channel.id, &incoming.text) {
            return Ok(());
        }
        if let Err(error) = self.notice(incoming, PAIRED_NOTICE).await {
            tracing::warn!(channel = %self.channel.id, ?error, "QQ pairing notice failed");
        }
        self.bots
            .add_allowed_sender(self.channel.id, incoming.sender.clone())
            .await
    }

    async fn admit(&self, incoming: Incoming) -> Result<(), ErrorCode> {
        if admit_waiting(self.bots, incoming.clone()).await? == Disposition::Busy
            && let Err(error) = self.notice(&incoming, BUSY_NOTICE).await
        {
            tracing::warn!(channel = %self.channel.id, ?error, "QQ busy notice failed");
        }
        Ok(())
    }

    async fn notice(&self, incoming: &Incoming, text: &str) -> Result<(), ErrorCode> {
        let path = message_path(&incoming.peer)?;
        let passive = incoming.reply_to.as_deref().map(|id| (id, 1));
        self.post_text(&path, text, passive).await
    }

    async fn reply(&self, turn: Turn, conversation: Conversation) -> Result<(), ErrorCode> {
        let parts = reply_parts(&turn);
        if parts.is_empty() {
            return Err(ErrorCode::BotIo);
        }
        let path = message_path(&conversation.peer)?;
        let mut index = 0;
        for text in parts {
            for chunk in chunks(text, MAX_CHUNK_UTF16) {
                let passive = turn
                    .reply_to
                    .as_deref()
                    .filter(|_| index < PASSIVE_REPLIES)
                    .map(|id| (id, index + 1));
                self.post_text(&path, chunk, passive).await?;
                index += 1;
            }
        }
        Ok(())
    }

    /// Send text as a reply when `passive` names the inbound message and its sequence number,
    /// falling back to a proactive message once QQ's reply window has closed.
    async fn post_text(
        &self,
        path: &str,
        text: &str,
        passive: Option<(&str, usize)>,
    ) -> Result<(), ErrorCode> {
        let proactive = json!({"content":text,"msg_type":0});
        if let Some((message, sequence)) = passive {
            let mut body = proactive.clone();
            body["msg_id"] = json!(message);
            body["msg_seq"] = json!(sequence);
            let response = self.call(reqwest::Method::POST, path, Some(&body)).await?;
            match refusal(&response) {
                None => return Ok(()),
                Some(code) if REPLY_WINDOW_CLOSED.contains(&code) => {}
                Some(_) => return Err(ErrorCode::BotIo),
            }
        }
        let response = self
            .call(reqwest::Method::POST, path, Some(&proactive))
            .await?;
        refusal(&response).map_or(Ok(()), |_| Err(ErrorCode::BotIo))
    }
}

fn refusal(response: &Value) -> Option<i64> {
    response["code"]
        .as_i64()
        .or_else(|| response["err_code"].as_i64())
        .filter(|code| *code != 0)
}

fn allowed(channel: &Channel, sender: &str) -> bool {
    matches!(&channel.config, ChannelConfig::Qq { allowed_senders, .. }
        if allowed_senders.iter().any(|allowed| allowed == sender))
}

/// The REST path for a stored conversation peer, refusing anything QQ did not issue.
fn message_path(peer: &str) -> Result<String, ErrorCode> {
    let (kind, id) = peer.split_once(':').ok_or(ErrorCode::BotIo)?;
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(ErrorCode::BotIo);
    }
    match kind {
        PRIVATE_PEER => Ok(format!("/v2/users/{id}/messages")),
        GROUP_PEER => Ok(format!("/v2/groups/{id}/messages")),
        _ => Err(ErrorCode::BotIo),
    }
}

/// Remove the leading mention QQ may leave at the start of a group message.
fn strip_mention(content: &str) -> &str {
    let content = content.trim();
    if let Some((_, rest)) = content
        .strip_prefix("<@")
        .and_then(|rest| rest.split_once('>'))
    {
        return rest.trim_start();
    }
    if content.starts_with('@') {
        return content
            .split_once(char::is_whitespace)
            .map_or("", |(_, rest)| rest.trim_start());
    }
    content
}

/// Parse a message event, without yet deciding whether its sender is allowed.
fn inbound(channel: &Channel, event: &str, data: &Value) -> Result<Option<Incoming>, ErrorCode> {
    let ChannelConfig::Qq { allow_groups, .. } = &channel.config else {
        return Err(ErrorCode::InvalidMessage);
    };
    let content = data["content"].as_str().unwrap_or_default();
    let (peer, sender, text) = match event {
        PRIVATE_MESSAGE => {
            let Some(sender) = data["author"]["user_openid"].as_str() else {
                return Ok(None);
            };
            (format!("{PRIVATE_PEER}:{sender}"), sender, content.trim())
        }
        GROUP_AT_MESSAGE | GROUP_ALL_MESSAGE if *allow_groups => {
            let Some(fields) = group_fields(data) else {
                return Ok(None);
            };
            fields
        }
        _ => return Ok(None),
    };
    let Some(message_id) = data["id"].as_str().filter(|id| !id.is_empty()) else {
        return Ok(None);
    };
    if text.is_empty() || text.len() > MAX_TEXT_BYTES {
        return Ok(None);
    }
    Ok(Some(Incoming {
        channel_id: channel.id,
        peer,
        sender: sender.to_owned(),
        thread: None,
        message_id: message_id.to_owned(),
        text: text.to_owned(),
        reply_to: Some(message_id.to_owned()),
    }))
}

/// Extract the common fields from a QQ group message.
fn group_fields(data: &Value) -> Option<(String, &str, &str)> {
    let group = data["group_openid"].as_str()?;
    let sender = data["author"]["member_openid"].as_str()?;
    Some((
        format!("{GROUP_PEER}:{group}"),
        sender,
        strip_mention(data["content"].as_str().unwrap_or_default()),
    ))
}

#[cfg(test)]
mod tests;
