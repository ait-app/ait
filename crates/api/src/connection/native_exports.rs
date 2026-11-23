//! Connection-scoped native export jobs and bounded cancellation receipts.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use model::Request;
use model::outbound::{Outbound, QueueError};
use model::server::{ClientMessage, ErrorCode, valid_id};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::{ConnectionSubscriptions, Incoming, Peer, error, process_message};
use crate::Shared;

const MAX_RECEIPTS: usize = 128;
const RECEIPT_TTL: Duration = Duration::from_secs(60);

#[derive(Debug)]
enum Status {
    Running(CancellationToken),
    Cancelled(Instant),
    Finished(Instant),
}

#[derive(Debug)]
struct Entry {
    agent: String,
    status: Status,
}

#[derive(Clone, Debug, Default)]
pub(super) struct NativeExports(Arc<Mutex<HashMap<String, Entry>>>);

pub(super) struct Lease {
    ledger: NativeExports,
    operation: String,
    cancel: CancellationToken,
}

impl Drop for Lease {
    fn drop(&mut self) {
        if let Ok(mut entries) = self.ledger.0.lock()
            && let Some(entry) = entries.get_mut(&self.operation)
            && matches!(entry.status, Status::Running(_))
        {
            entry.status = if self.cancel.is_cancelled() {
                Status::Cancelled(Instant::now())
            } else {
                Status::Finished(Instant::now())
            };
        }
    }
}

fn prune(entries: &mut HashMap<String, Entry>) {
    entries.retain(|_, entry| match entry.status {
        Status::Running(_) => true,
        Status::Cancelled(at) | Status::Finished(at) => at.elapsed() < RECEIPT_TTL,
    });
}

fn operation(value: &Value) -> Result<String, ErrorCode> {
    let id = value.as_str().ok_or(ErrorCode::InvalidMessage)?;
    let parsed = uuid::Uuid::parse_str(id).map_err(|_| ErrorCode::InvalidMessage)?;
    if parsed.get_version_num() != 4 || parsed.to_string() != id {
        return Err(ErrorCode::InvalidMessage);
    }
    Ok(id.to_owned())
}

pub(super) fn request_id(message: &Incoming) -> Option<String> {
    match message {
        Incoming::Text(ClientMessage::Request { request_id, .. }, _) => Some(request_id.clone()),
        _ => None,
    }
}

pub(super) fn is_start(message: &Incoming) -> bool {
    matches!(message, Incoming::Text(ClientMessage::Request { method, .. }, _) if method == "agent.native_html.export.request")
}

pub(super) fn is_cancel(request: &Request) -> bool {
    request.method == "agent.native_html.export.cancel.request"
        && request.params.get("operationId").is_some()
}

pub(super) fn is_cancel_message(message: &Incoming) -> bool {
    matches!(message, Incoming::Text(ClientMessage::Request { method, params, .. }, _) if method == "agent.native_html.export.cancel.request" && params.get("operationId").is_some())
}

impl NativeExports {
    /// Register before scheduling the job, preserving the physical connection's frame order.
    pub(super) fn admit(&self, message: &mut Incoming) -> Result<Lease, ErrorCode> {
        let Incoming::Text(
            ClientMessage::Request {
                request_id, params, ..
            },
            _,
        ) = message
        else {
            return Err(ErrorCode::InvalidMessage);
        };
        if !valid_id(request_id) {
            return Err(ErrorCode::InvalidMessage);
        }
        let agent = params
            .get("agentId")
            .and_then(Value::as_str)
            .filter(|id| valid_id(id))
            .ok_or(ErrorCode::InvalidMessage)?
            .to_owned();
        let id = operation(params.get("operationId").ok_or(ErrorCode::InvalidMessage)?)?;
        let mut entries = self.0.lock().map_err(|_| ErrorCode::AgentIo)?;
        prune(&mut entries);
        if let Some(entry) = entries.get(&id) {
            return Err(
                if entry.agent == agent && matches!(entry.status, Status::Cancelled(_)) {
                    ErrorCode::AgentIo
                } else {
                    ErrorCode::IdempotencyConflict
                },
            );
        }
        if entries.len() >= MAX_RECEIPTS {
            return Err(ErrorCode::ResourceExhausted);
        }
        let cancel = CancellationToken::new();
        entries.insert(
            id.clone(),
            Entry {
                agent,
                status: Status::Running(cancel.clone()),
            },
        );
        params
            .as_object_mut()
            .ok_or(ErrorCode::InvalidMessage)?
            .remove("operationId");
        Ok(Lease {
            ledger: self.clone(),
            operation: id,
            cancel,
        })
    }

    pub(super) fn cancel_request(
        &self,
        outbound: &Outbound,
        request: Request,
    ) -> Result<(), QueueError> {
        let result = self
            .cancel(&request.params)
            .map(|released| serde_json::json!({"released":released}));
        outbound.respond(request.id, result)
    }

    fn cancel(&self, params: &Value) -> Result<bool, ErrorCode> {
        let object = params.as_object().ok_or(ErrorCode::InvalidMessage)?;
        if object.len() != 2
            || !object.contains_key("agentId")
            || !object.contains_key("operationId")
        {
            return Err(ErrorCode::InvalidMessage);
        }
        let agent = object["agentId"]
            .as_str()
            .filter(|id| valid_id(id))
            .ok_or(ErrorCode::InvalidMessage)?;
        let id = operation(&object["operationId"])?;
        let mut entries = self.0.lock().map_err(|_| ErrorCode::AgentIo)?;
        prune(&mut entries);
        if let Some(entry) = entries.get_mut(&id) {
            if entry.agent != agent {
                return Ok(false);
            }
            return Ok(match &entry.status {
                Status::Running(token) => {
                    token.cancel();
                    true
                }
                Status::Cancelled(_) => true,
                Status::Finished(_) => false,
            });
        }
        if entries.len() >= MAX_RECEIPTS {
            return Err(ErrorCode::ResourceExhausted);
        }
        entries.insert(
            id,
            Entry {
                agent: agent.to_owned(),
                status: Status::Cancelled(Instant::now()),
            },
        );
        Ok(true)
    }
}

pub(super) struct JobContext<'a> {
    pub(super) state: &'a Arc<Shared>,
    pub(super) outbound: &'a Outbound,
    pub(super) peer: Peer<'a>,
    pub(super) ledger: NativeExports,
}

pub(super) async fn run(
    message: Incoming,
    lease: Lease,
    context: JobContext<'_>,
) -> Result<(), QueueError> {
    let request_id = request_id(&message);
    let mut subscriptions = ConnectionSubscriptions {
        native_exports: context.ledger,
        ..Default::default()
    };
    tokio::select! {
        biased;
        () = lease.cancel.cancelled() => error(context.outbound, request_id, ErrorCode::AgentIo),
        result = process_message(Some(Ok(message)), context.state, context.outbound, context.peer, &mut subscriptions) => result.map(|_| ()),
    }
}

#[cfg(test)]
mod tests;
