//! iMessage through Photon Beta 2.0: a project event stream in, REST sends out; see ADR-226.

pub(crate) mod onboarding;
mod socket_io;

use std::time::Duration;

use domain::bot::{Channel, ChannelConfig, Conversation, Turn};
use futures_util::{SinkExt, StreamExt};
use model::ErrorCode;
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::handshake::client::Request;
use tokio_tungstenite::tungstenite::http::header::{AUTHORIZATION, HeaderValue};
use tokio_util::sync::CancellationToken;

use self::socket_io::Packet;
use super::{
    BUSY_NOTICE, Disposition, Incoming, Socket, admit, chunks, connect_socket, deliver, expose,
    supervise,
};
use crate::service::Bots;

const API: &str = "https://api.photon.codes";
/// The event contract this adapter parses; every subscription pins it.
const API_VERSION: &str = "2026-07-01";
const MESSAGE_RECEIVED: &str = "message.received";
const PLATFORM: &str = "imessage";
const STATUS_RECEIVED: &str = "received";
const SPACE_DIRECT: &str = "direct";
const KIND_AGENT: &str = "agent";
const CONTENT_TEXT: &str = "text";
const MAX_TEXT_BYTES: usize = 16 * 1024;
const MAX_CHUNK_UTF16: usize = 3500;
const SUBSCRIBE_ACK: u64 = 0;
/// Longest silence allowed before the server has announced its own ping schedule.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
/// Keeps a slow notice from delaying the ping answers on the same connection.
const BUSY_NOTICE_TIMEOUT: Duration = Duration::from_secs(5);
/// Pause before reconnecting after a full Bot backlog refused a message.
const BACKLOG_WAIT: Duration = Duration::from_secs(5);

const PROJECT_PREFIX: &str = "pho_prj_";
const PROJECT_SUFFIX_LENGTH: usize = 26;

struct Connection<'a> {
    bots: &'a Bots,
    channel: &'a Channel,
    client: &'a reqwest::Client,
    key: &'a SecretString,
    api: &'a str,
    project_id: &'a str,
    backlog_wait: Duration,
}

/// Whether `project` is Photon's project ID shape, which is interpolated into request paths.
pub(crate) fn valid_project_id(project: &str) -> bool {
    project.strip_prefix(PROJECT_PREFIX).is_some_and(|suffix| {
        suffix.len() == PROJECT_SUFFIX_LENGTH
            && suffix.as_bytes()[0].is_ascii_digit()
            && suffix.as_bytes()[0] <= b'7'
            && suffix.bytes().all(|byte| {
                byte.is_ascii_digit()
                    || (byte.is_ascii_lowercase() && !matches!(byte, b'i' | b'l' | b'o' | b'u'))
            })
    })
}

/// The external account a project's event stream belongs to.
fn account_identity(project: &str) -> String {
    format!("photon:{project}")
}

pub(super) async fn run(
    bots: &Bots,
    channel: &Channel,
    client: &reqwest::Client,
    key: &SecretString,
    cancel: &CancellationToken,
) -> Result<(), ErrorCode> {
    run_at(bots, channel, (client, key, API), cancel).await
}

async fn run_at(
    bots: &Bots,
    channel: &Channel,
    transport: (&reqwest::Client, &SecretString, &str),
    cancel: &CancellationToken,
) -> Result<(), ErrorCode> {
    let ChannelConfig::PhotonImessage { project_id, .. } = &channel.config else {
        return Err(ErrorCode::InvalidMessage);
    };
    let (client, key, api) = transport;
    bots.account(channel.id, account_identity(project_id))
        .await?;
    let connection = Connection {
        bots,
        channel,
        client,
        key,
        api,
        project_id,
        backlog_wait: BACKLOG_WAIT,
    };
    let connection = &connection;
    let stop = cancel.child_token();
    let sending = deliver(bots, channel.id, &stop, |turn, conversation| async move {
        connection.reply(turn, conversation).await
    });
    // The socket makes this future large enough to be worth keeping off the stack.
    let receiving = Box::pin(connection.receive_all());
    supervise(cancel, &stop, receiving, sending).await
}

async fn send_text(socket: &mut Socket, text: String) -> Result<(), ErrorCode> {
    socket
        .send(WsMessage::Text(text.into()))
        .await
        .map_err(|_| ErrorCode::BotIo)
}

/// The next text frame, or `None` for a frame that needs no handling.
async fn next_text(socket: &mut Socket, limit: Duration) -> Result<Option<String>, ErrorCode> {
    let message = tokio::time::timeout(limit, socket.next())
        .await
        .map_err(|_| ErrorCode::BotIo)?
        .ok_or(ErrorCode::BotIo)?
        .map_err(|_| ErrorCode::BotIo)?;
    match message {
        WsMessage::Text(text) => Ok(Some(text.to_string())),
        WsMessage::Close(_) => Err(ErrorCode::BotIo),
        WsMessage::Binary(_) | WsMessage::Ping(_) | WsMessage::Pong(_) | WsMessage::Frame(_) => {
            Ok(None)
        }
    }
}

/// Build a Bot input from an event, or `None` for anything this channel does not accept.
fn incoming(channel: &Channel, event: &Value) -> Option<Incoming> {
    let ChannelConfig::PhotonImessage {
        allowed_senders, ..
    } = &channel.config
    else {
        return None;
    };
    let message = &event["data"]["object"];
    let accepted = event["type"] == MESSAGE_RECEIVED
        && message["platform"] == PLATFORM
        && message["status"] == STATUS_RECEIVED
        && message["sentBy"]["kind"] != KIND_AGENT
        && message["space"]["kind"] == SPACE_DIRECT
        && message["content"]["type"] == CONTENT_TEXT;
    if !accepted {
        return None;
    }
    let sender = message["sentBy"]["handle"].as_str()?;
    if !allowed_senders
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(sender))
    {
        return None;
    }
    let text = message["content"]["text"]
        .as_str()
        .filter(|text| !text.trim().is_empty() && text.len() <= MAX_TEXT_BYTES)?;
    if onboarding::is_pairing_code(text) {
        return None;
    }
    let (Some(id), Some(space)) = (message["id"].as_str(), message["space"]["id"].as_str()) else {
        return None;
    };
    let Some(agent) = message["receivedBy"]["id"].as_str() else {
        tracing::warn!(channel = %channel.id, "Photon message has no recipient to reply as");
        return None;
    };
    Some(Incoming {
        channel_id: channel.id,
        peer: format!("{space}/{agent}"),
        sender: sender.to_owned(),
        thread: None,
        message_id: id.to_owned(),
        text: text.to_owned(),
        reply_to: None,
    })
}

impl Connection<'_> {
    /// Keep the event stream open. A full backlog only replaces the connection, inside the
    /// receiver, so the delivery loop beside it can finish replies and drain that backlog.
    async fn receive_all(&self) -> Result<(), ErrorCode> {
        loop {
            let socket =
                tokio::time::timeout(HANDSHAKE_TIMEOUT, connect_socket(self.stream_request()?))
                    .await
                    .map_err(|_| ErrorCode::BotIo)??;
            // The channel snapshot is as old as this runner, so read the saved position.
            let resume = self.bots.channel_cursor(self.channel.id).await?;
            match self.receive(socket, resume).await {
                Err(ErrorCode::ResourceExhausted) => tokio::time::sleep(self.backlog_wait).await,
                settled => return settled,
            }
        }
    }

    fn stream_request(&self) -> Result<Request, ErrorCode> {
        // Maps both https to wss and, for tests, http to ws.
        let base = self.api.replacen("http", "ws", 1);
        let mut request = format!(
            "{base}/v1/projects/{}/events/ws?EIO=4&transport=websocket",
            self.project_id
        )
        .into_client_request()
        .map_err(|_| ErrorCode::BotIo)?;
        let mut value = HeaderValue::from_str(&format!("Bearer {}", expose(self.key)))
            .map_err(|_| ErrorCode::BotIo)?;
        value.set_sensitive(true);
        request.headers_mut().insert(AUTHORIZATION, value);
        Ok(request)
    }

    async fn receive(&self, mut socket: Socket, resume: Option<i64>) -> Result<(), ErrorCode> {
        let mut handled = resume;
        let mut limit = HANDSHAKE_TIMEOUT;
        let mut liveness = HANDSHAKE_TIMEOUT;
        loop {
            let Some(frame) = next_text(&mut socket, limit).await? else {
                continue;
            };
            match socket_io::decode(&frame)? {
                Packet::Open(window) => {
                    liveness = window;
                    send_text(&mut socket, socket_io::CONNECT.to_owned()).await?;
                }
                Packet::Connected => {
                    let subscribe = json!({
                        "startSeq":handled.unwrap_or(0),
                        "apiVersion":API_VERSION,
                    });
                    let frame = socket_io::event(SUBSCRIBE_ACK, "subscribe", &subscribe);
                    send_text(&mut socket, frame).await?;
                }
                Packet::Ack(SUBSCRIBE_ACK, answer) => {
                    self.subscribed(&answer).await?;
                    limit = liveness;
                }
                Packet::Ping => send_text(&mut socket, socket_io::PONG.to_owned()).await?,
                Packet::Event(arguments) => self.event(&arguments, &mut handled).await?,
                Packet::Closed => return Err(ErrorCode::BotIo),
                Packet::Ack(..) | Packet::Ignored => {}
            }
        }
    }

    async fn subscribed(&self, answer: &Value) -> Result<(), ErrorCode> {
        if answer["ok"] != true {
            tracing::warn!(
                channel = %self.channel.id,
                code = answer["error"]["code"].as_str().unwrap_or("unknown"),
                "Photon refused the event subscription"
            );
            return Err(ErrorCode::BotIo);
        }
        self.bots
            .channel_status(
                self.channel.id,
                json!({"state":"connected","accountId":self.project_id}),
            )
            .await;
        Ok(())
    }

    async fn event(&self, arguments: &[Value], handled: &mut Option<i64>) -> Result<(), ErrorCode> {
        match arguments {
            [name, event, metadata, ..] if name == "delivery" => {
                self.delivery(event, metadata, handled).await
            }
            [name, error, ..] if name == "delivery_error" => {
                tracing::warn!(
                    channel = %self.channel.id,
                    code = error["code"].as_str().unwrap_or("unknown"),
                    "Photon reported a delivery error"
                );
                if error["fatal"] == true {
                    return Err(ErrorCode::BotIo);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Handle one delivery and save its position only once its message is settled.
    async fn delivery(
        &self,
        event: &Value,
        metadata: &Value,
        handled: &mut Option<i64>,
    ) -> Result<(), ErrorCode> {
        let sequence = metadata["seq"].as_i64().ok_or(ErrorCode::BotIo)?;
        if handled.is_some_and(|last| sequence <= last) {
            return Ok(());
        }
        if let Some(message) = incoming(self.channel, event) {
            match admit(self.bots, message.clone()).await? {
                Disposition::Accepted | Disposition::Skip => {}
                Disposition::Busy => self.notify_busy(&message).await,
                // Waiting here would leave pings unanswered, so reconnect and resume instead.
                Disposition::Retry => return Err(ErrorCode::ResourceExhausted),
            }
        }
        self.bots.cursor(self.channel.id, sequence).await?;
        *handled = Some(sequence);
        Ok(())
    }

    /// Best-effort reply telling the sender their message was dropped; failures are only logged.
    async fn notify_busy(&self, message: &Incoming) {
        let idempotency = format!("busy:{}", message.message_id);
        let sent = tokio::time::timeout(
            BUSY_NOTICE_TIMEOUT,
            self.send(&message.peer, BUSY_NOTICE, &idempotency),
        )
        .await;
        if !matches!(sent, Ok(Ok(()))) {
            tracing::warn!(channel = %self.channel.id, "Photon busy notice failed");
        }
    }

    async fn reply(&self, turn: Turn, conversation: Conversation) -> Result<(), ErrorCode> {
        let text = turn.reply.as_deref().ok_or(ErrorCode::BotIo)?;
        self.send_chunks(&conversation.peer, &turn.id.to_string(), text)
            .await
    }

    /// Send a reply in parts, each with a key that stays stable if the same part is retried.
    async fn send_chunks(&self, peer: &str, turn: &str, text: &str) -> Result<(), ErrorCode> {
        for (index, chunk) in chunks(text, MAX_CHUNK_UTF16).into_iter().enumerate() {
            self.send(peer, chunk, &format!("{turn}:{index}")).await?;
        }
        Ok(())
    }

    /// Hand one message to Photon, which accepts it for asynchronous dispatch.
    async fn send(&self, peer: &str, text: &str, idempotency: &str) -> Result<(), ErrorCode> {
        let (space, sender) = peer.rsplit_once('/').ok_or(ErrorCode::BotIo)?;
        let response = self
            .client
            .post(format!(
                "{}/v1/projects/{}/messages",
                self.api, self.project_id
            ))
            .bearer_auth(expose(self.key))
            .header("idempotency-key", idempotency)
            .json(&json!({
                "spaceId":space,
                "senderId":sender,
                "content":{"type":CONTENT_TEXT,"format":"plain","text":text},
            }))
            .send()
            .await
            .map_err(|_| ErrorCode::BotIo)?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(ErrorCode::BotIo)
        }
    }
}

#[cfg(test)]
mod tests;
