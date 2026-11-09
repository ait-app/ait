//! The Socket.IO v5 over Engine.IO v4 subset Photon's event stream needs: text packets on the
//! default namespace, with no binary attachments or transport upgrades.

use std::time::Duration;

use model::ErrorCode;
use serde_json::{Value, json};

/// Asks to join the default namespace.
pub(super) const CONNECT: &str = "40";
/// Answers an Engine.IO ping.
pub(super) const PONG: &str = "3";

/// One decoded packet.
#[derive(Debug, PartialEq)]
pub(super) enum Packet {
    /// The transport is open; silence longer than this duration means the connection is dead.
    Open(Duration),
    /// The namespace accepted the connection.
    Connected,
    /// The server expects a [`PONG`].
    Ping,
    /// The answer to the numbered event this client sent.
    Ack(u64, Value),
    /// A server event's name and arguments, as one array.
    Event(Vec<Value>),
    /// The server ended the transport, the namespace, or refused the connection.
    Closed,
    /// Needs no handling.
    Ignored,
}

/// Decode one text frame; a packet outside the supported subset is a protocol failure.
pub(super) fn decode(frame: &str) -> Result<Packet, ErrorCode> {
    let (kind, body) = split_kind(frame)?;
    match kind {
        b'0' => open(body),
        b'1' => Ok(Packet::Closed),
        b'2' => Ok(Packet::Ping),
        b'3' | b'5' | b'6' => Ok(Packet::Ignored),
        b'4' => message(body),
        _ => Err(ErrorCode::BotIo),
    }
}

fn split_kind(text: &str) -> Result<(u8, &str), ErrorCode> {
    let kind = *text.as_bytes().first().ok_or(ErrorCode::BotIo)?;
    // A leading ASCII digit is one byte, so anything else is rejected before slicing.
    if !kind.is_ascii_digit() {
        return Err(ErrorCode::BotIo);
    }
    Ok((kind, &text[1..]))
}

fn open(body: &str) -> Result<Packet, ErrorCode> {
    let handshake: Value = serde_json::from_str(body).map_err(|_| ErrorCode::BotIo)?;
    let interval = handshake["pingInterval"].as_u64().ok_or(ErrorCode::BotIo)?;
    let timeout = handshake["pingTimeout"].as_u64().ok_or(ErrorCode::BotIo)?;
    Ok(Packet::Open(Duration::from_millis(
        interval.saturating_add(timeout),
    )))
}

fn message(body: &str) -> Result<Packet, ErrorCode> {
    let (kind, body) = split_kind(body)?;
    match kind {
        b'0' => Ok(Packet::Connected),
        b'1' | b'4' => Ok(Packet::Closed),
        b'2' => Ok(Packet::Event(arguments(body)?.1)),
        b'3' => {
            let (id, values) = arguments(body)?;
            let id = id.ok_or(ErrorCode::BotIo)?;
            let value = values.into_iter().next().ok_or(ErrorCode::BotIo)?;
            Ok(Packet::Ack(id, value))
        }
        _ => Err(ErrorCode::BotIo),
    }
}

/// Split an optional ack number from the JSON array that follows it.
fn arguments(body: &str) -> Result<(Option<u64>, Vec<Value>), ErrorCode> {
    let digits = body.bytes().take_while(u8::is_ascii_digit).count();
    let (id, array) = body.split_at(digits);
    let id = (!id.is_empty())
        .then(|| id.parse().map_err(|_| ErrorCode::BotIo))
        .transpose()?;
    let values = serde_json::from_str(array).map_err(|_| ErrorCode::BotIo)?;
    Ok((id, values))
}

/// Encode an event that expects the numbered answer.
pub(super) fn event(ack: u64, name: &str, argument: &Value) -> String {
    format!("42{ack}{}", json!([name, argument]))
}

#[cfg(test)]
mod tests;
