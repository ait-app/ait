//! One optional, epoch-scoped WSS tunnel shared by desktop and headless Hosts.

mod bridge;
mod mux;
mod wire;

use std::io;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt, future};
use secrecy::ExposeSecret;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio::time::{Instant, sleep, timeout};
use tokio_tungstenite::tungstenite::Message;
use url::Url;
use uuid::Uuid;

use crate::protocol::{TunnelGrant, TunnelRequest};
use crate::{Local, endpoint, protocol, transport};

pub(super) const PROTOCOL: &str = wire::PROTOCOL;
pub(super) type GrantResult = (Uuid, Option<TunnelGrant>);

/// Maintain the optional data tunnel for one control epoch until its owner cancels the task.
/// Failed grants/dials back off independently and leave dedicated visits available.
pub(super) async fn maintain(
    local: Arc<Local>,
    center: Url,
    epoch: Uuid,
    requests: mpsc::Sender<TunnelRequest>,
    mut grants: mpsc::Receiver<GrantResult>,
) {
    let mut failures = 0_u32;
    loop {
        let request_id = Uuid::new_v4();
        let started = Instant::now();
        if requests
            .send(TunnelRequest {
                request_id,
                protocol: PROTOCOL,
                epoch,
            })
            .await
            .is_err()
        {
            return;
        }
        let grant = timeout(Duration::from_secs(5), async {
            while let Some((id, grant)) = grants.recv().await {
                if id == request_id {
                    return grant;
                }
            }
            None
        })
        .await
        .ok()
        .flatten();
        if let Some(grant) = grant
            && grant.epoch == epoch
            && grant.protocol == PROTOCOL
            && grant.tunnel_ticket.expose_secret().len() == 64
            && grant
                .tunnel_ticket
                .expose_secret()
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            && run(local.clone(), &center, grant).await.is_err()
        {
            tracing::debug!(%epoch, "Relay data tunnel ended; dedicated relay remains available");
        }
        if started.elapsed() >= Duration::from_secs(60) {
            failures = 0;
        }
        let backoff_ms = (500_u64 << failures.min(5)).min(10_000);
        let jitter_ms = u64::from(request_id.as_bytes()[0]);
        failures = failures.saturating_add(1);
        sleep(Duration::from_millis(backoff_ms + jitter_ms)).await;
    }
}

async fn run(local: Arc<Local>, center: &Url, grant: TunnelGrant) -> io::Result<()> {
    let mut socket = local
        .dialer
        .connect(
            &endpoint(center, "v1/relay/tunnel"),
            grant.tunnel_ticket.expose_secret(),
            wire::MAX_BLOCK,
        )
        .await
        .map_err(|_| wire::invalid())?;
    let hello = wire::Prelude {
        protocol: PROTOCOL.to_owned(),
        epoch: grant.epoch,
        tunnel_id: grant.tunnel_id,
    };
    transport::send(
        &mut socket,
        Message::Text(
            protocol::encode(&hello)
                .map_err(|_| wire::invalid())?
                .into(),
        ),
    )
    .await
    .map_err(|_| wire::invalid())?;
    let welcome: wire::Prelude = timeout(Duration::from_secs(5), transport::receive(&mut socket))
        .await
        .map_err(|_| wire::invalid())?
        .map_err(|_| wire::invalid())?;
    if welcome.protocol != PROTOCOL
        || welcome.epoch != grant.epoch
        || welcome.tunnel_id != grant.tunnel_id
    {
        return Err(wire::invalid());
    }
    let (sink, stream) = socket.split();
    let sink = sink
        .sink_map_err(|_| wire::invalid())
        .with(|frame| future::ready(Ok(to_message(frame))));
    let stream = stream.map(|message| match message.map_err(|_| wire::invalid())? {
        Message::Binary(body) => Ok(mux::Frame::Binary(body.to_vec())),
        Message::Ping(body) => Ok(mux::Frame::Ping(body.to_vec())),
        Message::Pong(body) => Ok(mux::Frame::Pong(body.to_vec())),
        Message::Close(_) => Ok(mux::Frame::Close),
        Message::Text(_) | Message::Frame(_) => Err(wire::invalid()),
    });
    let (network, yamux) = tokio::io::duplex(wire::MAX_BLOCK);
    let (_outbound, requests) = mpsc::channel(1);
    let (incoming, mut streams) = mpsc::channel(16);
    let visits = async {
        let mut tasks = JoinSet::new();
        loop {
            tokio::select! {
                _ = tasks.join_next(), if !tasks.is_empty() => {},
                stream = streams.recv() => {
                    let Some(stream) = stream else { return Ok(()); };
                    let Ok(permit) = local.visits.clone().try_acquire_owned() else { drop(stream); continue; };
                    let local = local.clone();
                    let epoch = grant.epoch;
                    let tunnel_id = grant.tunnel_id;
                    tasks.spawn(async move {
                        let _permit = permit;
                        if bridge::visit(stream, local, epoch, tunnel_id).await.is_err() {
                            tracing::debug!(%tunnel_id, "Relay tunnel visit ended");
                        }
                    });
                },
            }
        }
    };
    tokio::select! {
        result = mux::transport(sink, stream, network) => result,
        result = mux::drive(yamux, yamux::Mode::Server, requests, Some(incoming)) => result,
        result = visits => result,
    }
}

fn to_message(frame: mux::Frame) -> Message {
    match frame {
        mux::Frame::Binary(body) => Message::Binary(body.into()),
        mux::Frame::Ping(body) => Message::Ping(body.into()),
        mux::Frame::Pong(body) => Message::Pong(body.into()),
        mux::Frame::Close => Message::Close(None),
    }
}

#[cfg(test)]
mod tests;
