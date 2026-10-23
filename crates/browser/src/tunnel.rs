//! Connection-owned, bounded byte streams originating from the Workspace host.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use base64::{Engine, engine::general_purpose::STANDARD};
use model::methods::MethodSpec;
use model::{Context, ErrorCode};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::Mutex;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

const MAX_CHUNK_BYTES: usize = 64 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) const METHODS: &[MethodSpec] = &[
    MethodSpec::request("browser.tunnel.open.request"),
    MethodSpec::request("browser.tunnel.read.request"),
    MethodSpec::request("browser.tunnel.write.request"),
];

#[derive(Debug, Default)]
/// TCP leases owned exclusively by one authenticated physical connection.
pub(crate) struct Tunnels(BTreeMap<String, Lease>);

#[derive(Debug)]
struct Lease(Arc<Stream>);

#[derive(Debug, Default)]
struct Stream {
    read: Mutex<Option<OwnedReadHalf>>,
    write: Mutex<Option<OwnedWriteHalf>>,
    cancellation: CancellationToken,
    closed: AtomicBool,
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.0.cancellation.cancel();
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Open {
    host: String,
    port: u16,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Read {
    subscription_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Write {
    subscription_id: String,
    data_base64: String,
    #[serde(default)]
    eof: bool,
}

impl Tunnels {
    /// Count live leases against the connection's shared subscription budget.
    pub(crate) fn len(&self) -> usize {
        self.0
            .values()
            .filter(|lease| !lease.0.closed.load(Ordering::Acquire))
            .count()
    }

    /// Release `id` and cancel pending I/O; unknown identities have no effect.
    pub(crate) fn release(&mut self, id: &str) {
        self.0.remove(id);
    }

    /// Admit one bounded operation without blocking the connection's request worker.
    ///
    /// # Arguments
    /// * `context` - Validated request with connection output and runtime ownership.
    ///
    /// # Returns
    /// Schedules I/O or delivers an immediate validation error.
    ///
    /// # Errors
    /// Returns delivery failures; asynchronous I/O errors use the correlated response.
    pub(crate) fn dispatch(
        &mut self,
        mut context: Context<'_>,
    ) -> Result<(), model::DispatchError> {
        if context.runtime.cancellation.is_cancelled() {
            return context
                .respond(Err(ErrorCode::ServerDraining))
                .map_err(Into::into);
        }
        let prepared = self.prepare(&mut context);
        let Ok((id, stream, operation)) = prepared else {
            return context
                .respond(prepared.map(|_| Value::Null))
                .map_err(Into::into);
        };
        let outbound = context.outbound.clone();
        let stopped = context.runtime.cancellation.clone();
        let request_id = context.request.id;
        context.runtime.tasks.spawn(async move {
            let failure = outbound.failure();
            let result = tokio::select! {
                () = stopped.cancelled() => Err(ErrorCode::ServerDraining),
                () = failure.cancelled() => Err(ErrorCode::SubscriptionNotFound),
                () = stream.cancellation.cancelled() => Err(ErrorCode::SubscriptionNotFound),
                result = execute(&stream, &id, operation) => result,
            };
            if result.is_err() {
                stream.closed.store(true, Ordering::Release);
                stream.cancellation.cancel();
                *stream.read.lock().await = None;
                *stream.write.lock().await = None;
            }
            let _ = outbound.respond(request_id, result);
        });
        Ok(())
    }

    fn prepare(
        &mut self,
        context: &mut Context<'_>,
    ) -> Result<(String, Arc<Stream>, Operation), ErrorCode> {
        let params = std::mem::take(&mut context.request.params);
        match context.request.method.as_str() {
            "browser.tunnel.open.request" => {
                let input: Open =
                    serde_json::from_value(params).map_err(|_| ErrorCode::InvalidMessage)?;
                let host = tunnel_host(&input.host)?;
                if input.port == 0 {
                    return Err(ErrorCode::InvalidMessage);
                }
                self.0
                    .retain(|_, lease| !lease.0.closed.load(Ordering::Acquire));
                if context.available_subscriptions == 0 {
                    return Err(ErrorCode::ResourceExhausted);
                }
                let id = uuid::Uuid::new_v4().to_string();
                let stream = Arc::new(Stream::default());
                self.0.insert(id.clone(), Lease(stream.clone()));
                Ok((id, stream, Operation::Open(host, input.port)))
            }
            "browser.tunnel.read.request" => {
                let input: Read =
                    serde_json::from_value(params).map_err(|_| ErrorCode::InvalidMessage)?;
                Ok((
                    input.subscription_id.clone(),
                    self.get(&input.subscription_id)?,
                    Operation::Read,
                ))
            }
            "browser.tunnel.write.request" => {
                let input: Write =
                    serde_json::from_value(params).map_err(|_| ErrorCode::InvalidMessage)?;
                if input.data_base64.len() > MAX_CHUNK_BYTES.div_ceil(3) * 4 {
                    return Err(ErrorCode::InvalidMessage);
                }
                let data = STANDARD
                    .decode(&input.data_base64)
                    .map_err(|_| ErrorCode::InvalidMessage)?;
                if data.len() > MAX_CHUNK_BYTES {
                    return Err(ErrorCode::InvalidMessage);
                }
                Ok((
                    input.subscription_id.clone(),
                    self.get(&input.subscription_id)?,
                    Operation::Write(data, input.eof),
                ))
            }
            _ => Err(ErrorCode::MethodNotFound),
        }
    }

    fn get(&self, id: &str) -> Result<Arc<Stream>, ErrorCode> {
        self.0
            .get(id)
            .filter(|lease| !lease.0.closed.load(Ordering::Acquire))
            .map(|lease| lease.0.clone())
            .ok_or(ErrorCode::SubscriptionNotFound)
    }
}

fn tunnel_host(host: &str) -> Result<String, ErrorCode> {
    if let Ok(address) = host.parse::<IpAddr>() {
        return Ok(address.to_string());
    }
    match url::Host::parse(host).map_err(|_| ErrorCode::InvalidMessage)? {
        url::Host::Domain(name) => {
            let hostname = name.trim_end_matches('.');
            if hostname.is_empty() || name.len() > 253 {
                return Err(ErrorCode::InvalidMessage);
            }
            if hostname == "localhost" || hostname.ends_with(".localhost") {
                Ok("127.0.0.1".into())
            } else {
                Ok(name)
            }
        }
        url::Host::Ipv4(address) => Ok(address.to_string()),
        url::Host::Ipv6(address) => Ok(address.to_string()),
    }
}

#[derive(Debug)]
enum Operation {
    Open(String, u16),
    Read,
    Write(Vec<u8>, bool),
}

async fn execute(stream: &Stream, id: &str, operation: Operation) -> Result<Value, ErrorCode> {
    match operation {
        Operation::Open(host, port) => {
            // Resolve names on the daemon, within the same deadline as the TCP connection.
            let socket = timeout(IO_TIMEOUT, TcpStream::connect((host.as_str(), port)))
                .await
                .map_err(|_| ErrorCode::DaemonIo)?
                .map_err(|_| ErrorCode::DaemonIo)?;
            socket.set_nodelay(true).map_err(|_| ErrorCode::DaemonIo)?;
            let (read, write) = socket.into_split();
            *stream.read.lock().await = Some(read);
            *stream.write.lock().await = Some(write);
            Ok(json!({"subscriptionId": id}))
        }
        Operation::Read => {
            let mut guard = stream
                .read
                .try_lock()
                .map_err(|_| ErrorCode::ResourceExhausted)?;
            let read = guard.as_mut().ok_or(ErrorCode::SubscriptionNotFound)?;
            let mut data = vec![0; MAX_CHUNK_BYTES];
            let count = match timeout(Duration::from_secs(30), read.read(&mut data)).await {
                Ok(result) => result.map_err(|_| ErrorCode::DaemonIo)?,
                Err(_) => return Ok(json!({"dataBase64": "", "eof": false})),
            };
            Ok(json!({"dataBase64": STANDARD.encode(&data[..count]), "eof": count == 0}))
        }
        Operation::Write(data, eof) => {
            let mut guard = stream
                .write
                .try_lock()
                .map_err(|_| ErrorCode::ResourceExhausted)?;
            let write = guard.as_mut().ok_or(ErrorCode::SubscriptionNotFound)?;
            timeout(IO_TIMEOUT, async {
                write.write_all(&data).await?;
                if eof {
                    write.shutdown().await?;
                }
                Ok::<_, std::io::Error>(())
            })
            .await
            .map_err(|_| ErrorCode::DaemonIo)?
            .map_err(|_| ErrorCode::DaemonIo)?;
            Ok(json!({}))
        }
    }
}

#[cfg(test)]
mod tests;
