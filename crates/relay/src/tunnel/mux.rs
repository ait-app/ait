//! Bounded transport adapter and continuously polled Yamux connection driver.

use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::Poll;
use std::time::Duration;

use futures_util::{Sink, SinkExt, Stream, StreamExt, future::poll_fn};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{Instant, timeout};
use tokio_util::compat::TokioAsyncReadCompatExt;

use super::wire::{MAX_BLOCK, invalid};

/// One caller waiting for an outbound stream from the continuously polled driver.
pub(super) type OpenRequest = oneshot::Sender<yamux::Stream>;

#[derive(Debug)]
/// Hop-local WebSocket events, independent of the WebSocket implementation.
pub(super) enum Frame {
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close,
}

/// Bound a write or setup operation to five seconds; propagate its I/O error.
pub(super) async fn bounded<T>(future: impl Future<Output = io::Result<T>>) -> io::Result<T> {
    timeout(Duration::from_secs(5), future)
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Relay tunnel write timed out"))?
}

/// Bridge bounded binary WebSocket blocks to a byte stream with 10s/30s liveness.
///
/// Either direction ending cancels the other; stalled writes and invalid blocks fail the tunnel.
pub(super) async fn transport<S, R>(sink: S, stream: R, bytes: DuplexStream) -> io::Result<()>
where
    S: Sink<Frame, Error = io::Error> + Unpin,
    R: Stream<Item = io::Result<Frame>> + Unpin,
{
    let (mut sink, mut stream) = (sink, stream);
    let (mut reader, mut writer) = tokio::io::split(bytes);
    let (pong_tx, mut pong_rx) = mpsc::channel(1);
    let (alive_tx, alive_rx) = watch::channel(Instant::now());
    let nonce = AtomicU64::new(0);
    let receive = async {
        while let Some(frame) = stream.next().await {
            match frame? {
                Frame::Binary(body) if body.len() <= MAX_BLOCK => {
                    bounded(writer.write_all(&body)).await?;
                }
                Frame::Ping(body) => pong_tx.try_send(Frame::Pong(body)).map_err(|_| invalid())?,
                Frame::Pong(body) => {
                    if body == nonce.load(Ordering::Relaxed).to_be_bytes() {
                        alive_tx.send_replace(Instant::now());
                    }
                }
                Frame::Close => return Ok(()),
                Frame::Binary(_) => return Err(invalid()),
            }
        }
        Ok(())
    };
    let send = async {
        let mut block = vec![0; 16 * 1024];
        let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            let frame = tokio::select! {
                biased;
                _ = heartbeat.tick() => {
                    if alive_rx.borrow().elapsed() >= Duration::from_secs(30) {
                        return Err(io::Error::new(io::ErrorKind::TimedOut, "Tunnel heartbeat expired"));
                    }
                    let next = nonce.fetch_add(1, Ordering::Relaxed) + 1;
                    Frame::Ping(next.to_be_bytes().to_vec())
                },
                Some(pong) = pong_rx.recv() => pong,
                read = reader.read(&mut block) => {
                    let length = read?;
                    if length == 0 { return Ok(()); }
                    Frame::Binary(block[..length].to_vec())
                },
            };
            bounded(sink.send(frame)).await?;
        }
    };
    tokio::select! {
        result = receive => result,
        result = send => result,
    }
}

/// Continuously drive frames and flow-control updates, servicing bounded stream requests.
///
/// Centers reject inbound streams. Hosts accept only as many streams as their bounded queue holds.
/// Connection errors stop all streams; dropping one stream leaves other visits running.
pub(super) async fn drive(
    bytes: DuplexStream,
    mode: yamux::Mode,
    mut requests: mpsc::Receiver<OpenRequest>,
    incoming: Option<mpsc::Sender<yamux::Stream>>,
) -> io::Result<()> {
    let mut config = yamux::Config::default();
    config.set_max_num_streams(16);
    config.set_max_connection_receive_window(Some(4 * 1024 * 1024));
    config.set_split_send_size(16 * 1024);
    config.set_read_after_close(false);
    let mut connection = yamux::Connection::new(bytes.compat(), config, mode);
    let mut pending: Option<OpenRequest> = None;
    loop {
        poll_fn(|cx| {
            match connection.poll_next_inbound(cx) {
                Poll::Ready(Some(Ok(stream))) => {
                    let Some(sender) = &incoming else {
                        return Poll::Ready(Err(invalid()));
                    };
                    // A slow consumer cannot stop the connection/window driver.
                    let _ = sender.try_send(stream);
                    return Poll::Ready(Ok(()));
                }
                Poll::Ready(Some(Err(_)) | None) => return Poll::Ready(Err(invalid())),
                Poll::Pending => {}
            }
            if pending.is_none()
                && let Poll::Ready(Some(request)) = requests.poll_recv(cx)
            {
                pending = Some(request);
            }
            if let Some(request) = &pending {
                if request.is_closed() {
                    pending = None;
                    return Poll::Ready(Ok(()));
                }
                match connection.poll_new_outbound(cx) {
                    Poll::Ready(Ok(stream)) => {
                        if let Some(request) = pending.take() {
                            let _ = request.send(stream);
                        }
                        return Poll::Ready(Ok(()));
                    }
                    Poll::Ready(Err(_)) => return Poll::Ready(Err(invalid())),
                    Poll::Pending => {}
                }
            }
            Poll::Pending
        })
        .await?;
    }
}

#[cfg(test)]
#[path = "mux/tests.rs"]
mod tests;
