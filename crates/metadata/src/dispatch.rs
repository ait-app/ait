//! Concrete service state and crate-owned request dispatch.

use std::sync::{Arc, Mutex};

use model::outbound::QueueError;
use model::{Context, DispatchError, ErrorCode, Runtime};

mod requests;

/// Services installed for this capability crate, sharing server-wide runtime resources.
#[derive(Debug)]
pub struct State {
    /// Installed persistent push token leases.
    pub push_tokens: Option<Arc<Mutex<crate::service::push::PushTokens>>>,
    /// Shared Tokio admission, cancellation and task tracking.
    pub runtime: Arc<Runtime>,
    /// Installed daemon service.
    pub daemon: Option<Arc<Mutex<crate::service::daemon::Daemon>>>,
    /// Installed directory service.
    pub directory: Option<Arc<Mutex<crate::service::directory::Directory>>>,
    /// Installed workspace labels service.
    pub workspace_labels: Option<Arc<Mutex<crate::service::workspace_labels::WorkspaceLabels>>>,
    /// Installed workspace automation service.
    pub workspace_automation:
        Option<Arc<Mutex<crate::service::workspace_automation::WorkspaceAutomation>>>,
    /// Installed workspace state service.
    pub workspace_state: Option<Arc<Mutex<crate::service::workspace_state::WorkspaceState>>>,
    /// Durable progress of Workspace and native Agent creation.
    pub creations: model::creation::Creations,
    /// Shared ephemeral session event hub.
    pub session_events: model::session::SessionEvents,
    /// Whether Agent attention event production is installed.
    pub has_agent_execution: bool,
    /// Whether terminal hook attention events are installed.
    pub has_terminals: bool,
    /// Whether background Git observations and checkout state events are installed.
    pub has_git_fetch: bool,
}

impl std::ops::Deref for State {
    type Target = Runtime;

    fn deref(&self) -> &Runtime {
        &self.runtime
    }
}

use model::subscription::SubscriptionReleaseRequest;
use model::{Lifecycle, LifecycleIntent, ServerMessage, valid_id};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::connection::Connection;

/// Connection-wide work that follows metadata dispatch.
pub enum Completion {
    /// Ask the API composition layer for the Provider owner's availability snapshot.
    DaemonSnapshot {
        /// Original correlation identifier.
        request_id: String,
        /// Canonical status or diagnostics operation.
        method: String,
        /// Validated request parameters.
        params: Value,
    },
    /// Release this connection's matching observers in every capability crate, then acknowledge.
    Release {
        /// Correlation ID of the admitted release request.
        request_id: String,
        /// Validated connection-local subscription ID.
        subscription_id: String,
    },
}

impl State {
    /// Request a process lifecycle transition under the common admission lock.
    pub fn request_lifecycle(&self, intent: LifecycleIntent) {
        let admission = self
            .admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut requested = self
            .lifecycle_intent
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if requested.is_none() {
            *requested = Some(intent);
        }
        self.start_draining();
        drop(requested);
        drop(admission);
    }

    /// Publish the first draining transition and close task admission.
    pub fn start_draining(&self) {
        if !self.cancellation.is_cancelled() {
            let mut info = self.info();
            info.lifecycle = Lifecycle::Draining;
            self.session_events.publish(
                model::session::protocol::SessionEventKind::ServerInfo,
                &json!({"status":"server_info","info":info}),
            );
        }
        self.cancellation.cancel();
        self.tasks.close();
    }
}

/// Dispatch metadata requests and reusable API connection protocol operations.
/// This crate owns connection method declarations; those operations require no metadata service.
/// Leaves `context` unchanged for other crates; takes it when this crate handles the method.
/// Returns `DispatchError::NotImplemented` while leaving an unmatched Context available.
/// Returns optional work for the API to finish before ending request processing.
///
/// # Arguments
/// * `context` - Pending request, consumed only when this crate recognizes its method.
/// * `state` - Installed services and resources used to execute the request.
/// * `connection` - Connection-owned subscriptions and streams for this capability.
///
/// # Errors
/// Returns `NotImplemented` for an unmatched method and `Delivery` for an outbound failure.
/// Business errors are sent using the request's correlation ID.
pub async fn dispatch(
    context: &mut Option<Context<'_>>,
    state: &State,
    connection: &mut Connection,
) -> Result<Option<Completion>, DispatchError> {
    if context.is_none() {
        return Ok(None);
    }
    match base(context, state, connection) {
        Ok(completion) => return Ok(completion),
        Err(DispatchError::NotImplemented) => Context::assert_unhandled(context, "metadata::base"),
        Err(error) => return Err(error),
    }
    match requests::push(context, state, connection).await {
        Ok(()) => return Ok(None),
        Err(DispatchError::NotImplemented) => Context::assert_unhandled(context, "metadata::push"),
        Err(error) => return Err(error),
    }
    match requests::editor(context, state) {
        Ok(()) => return Ok(None),
        Err(DispatchError::NotImplemented) => {
            Context::assert_unhandled(context, "metadata::editor");
        }
        Err(error) => return Err(error),
    }
    match requests::creation(context, state, connection).await {
        Ok(()) => return Ok(None),
        Err(DispatchError::NotImplemented) => {
            Context::assert_unhandled(context, "metadata::creation");
        }
        Err(error) => return Err(error),
    }
    match requests::session(context, state, connection) {
        Ok(()) => return Ok(None),
        Err(DispatchError::NotImplemented) => {
            Context::assert_unhandled(context, "metadata::session");
        }
        Err(error) => return Err(error),
    }
    match requests::directory(context, state, connection).await {
        Ok(()) => return Ok(None),
        Err(DispatchError::NotImplemented) => {
            Context::assert_unhandled(context, "metadata::directory");
        }
        Err(error) => return Err(error),
    }
    match requests::daemon(context, state).await {
        Ok(completion) => return Ok(completion),
        Err(DispatchError::NotImplemented) => {
            Context::assert_unhandled(context, "metadata::daemon");
        }
        Err(error) => return Err(error),
    }
    match labels(context, state, connection).await {
        Ok(()) => return Ok(None),
        Err(DispatchError::NotImplemented) => {
            Context::assert_unhandled(context, "metadata::labels");
        }
        Err(error) => return Err(error),
    }
    match requests::automation(context, state).await {
        Ok(()) => return Ok(None),
        Err(DispatchError::NotImplemented) => {
            Context::assert_unhandled(context, "metadata::automation");
        }
        Err(error) => return Err(error),
    }
    match requests::workspace_state(context, state).await {
        Ok(()) => return Ok(None),
        Err(DispatchError::NotImplemented) => {
            Context::assert_unhandled(context, "metadata::workspace_state");
        }
        Err(error) => return Err(error),
    }
    Err(DispatchError::NotImplemented)
}

async fn labels(
    pending: &mut Option<Context<'_>>,
    state: &State,
    connection: &mut Connection,
) -> Result<(), DispatchError> {
    let Some(mut context) = pending.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "workspace.label.list.request"
                | "workspace.label.assignment.set.request"
                | "workspace.label.update.request"
                | "workspace.label.delete.inspect.request"
                | "workspace.label.delete.request"
        )
    }) else {
        return Err(DispatchError::NotImplemented);
    };

    if context.request.method == "workspace.label.list.request"
        && context
            .request
            .params
            .get("subscribe")
            .is_some_and(|value| !value.is_null())
        && context.available_subscriptions == 0
    {
        return context
            .respond(Err(ErrorCode::ResourceExhausted))
            .map_err(Into::into);
    }
    let params = std::mem::take(&mut context.request.params);
    let reply = crate::connection::workspace_labels::dispatch(
        &context.request.method,
        params,
        state,
        context.outbound.clone(),
    )
    .await;
    match reply {
        Ok(reply) => {
            context.respond(Ok(reply.value))?;
            if let Some(pending) = reply.subscription {
                let (id, subscription) = pending.activate().map_err(|error| match error {
                    crate::rpc::workspace_labels::DeliveryError::Encode(error) => {
                        QueueError::Encode(error)
                    }
                    crate::rpc::workspace_labels::DeliveryError::Closed => QueueError::Full,
                })?;
                connection.labels.insert(id, subscription);
            }
            Ok(())
        }
        Err(error) => context.respond(Err(error)).map_err(Into::into),
    }
}

fn base(
    pending: &mut Option<Context<'_>>,
    state: &State,
    connection: &mut Connection,
) -> Result<Option<Completion>, DispatchError> {
    let Some(context) = pending.take_if(|context| {
        matches!(
            context.request.method.as_str(),
            "server.info"
                | "connection.ping"
                | "server.status.subscribe"
                | "subscription.release.request"
                | "server.status.unsubscribe"
        )
    }) else {
        return Err(DispatchError::NotImplemented);
    };

    if context.request.method == "subscription.release.request" {
        let request =
            serde_json::from_value::<SubscriptionReleaseRequest>(context.request.params.clone());
        if let Ok(request) = request
            && valid_id(&request.subscription_id)
        {
            return Ok(Some(Completion::Release {
                request_id: context.request.id,
                subscription_id: request.subscription_id,
            }));
        }
        context.respond(Err(ErrorCode::InvalidMessage))?;
        return Ok(None);
    }
    let mut status = None;
    let result = (|| {
        Ok(match context.request.method.as_str() {
            "server.info" => {
                serde_json::to_value(state.info()).map_err(|_| ErrorCode::InvalidMessage)?
            }
            "connection.ping" => crate::rpc::server::ping(context.request.params.clone())?,
            "server.status.subscribe" => {
                if context.available_subscriptions == 0 {
                    return Err(ErrorCode::ResourceExhausted);
                }
                let id = Uuid::new_v4().to_string();
                connection.status.insert(id.clone());
                status = Some(id.clone());
                json!({"subscription_id":id})
            }
            "server.status.unsubscribe" => {
                let id = context
                    .request
                    .params
                    .get("subscription_id")
                    .and_then(Value::as_str)
                    .ok_or(ErrorCode::InvalidMessage)?;
                if !connection.status.remove(id) {
                    return Err(ErrorCode::SubscriptionNotFound);
                }
                json!({"unsubscribed":true})
            }
            _ => return Err(ErrorCode::MethodNotFound),
        })
    })();
    let outbound = context.outbound;
    context.respond(result)?;
    if let Some(subscription_id) = status {
        outbound.send(&ServerMessage::Status {
            subscription_id,
            lifecycle: state.info().lifecycle,
        })?;
    }
    Ok(None)
}

/// Finish a daemon snapshot using live availability obtained by the API from Provider.
/// # Errors
/// Returns admission, missing-service, validation or serialization errors.
pub async fn daemon_snapshot(
    state: &State,
    method: String,
    params: Value,
    providers: Vec<crate::protocol::daemon::ProviderAvailability>,
) -> Result<Value, ErrorCode> {
    let capabilities = state.info.implemented_capabilities.clone();
    let lifecycle = state.info().lifecycle;
    state
        .run(state.daemon.clone(), ErrorCode::DaemonIo, move |daemon| {
            crate::rpc::daemon::snapshot(
                daemon,
                &method,
                params,
                &providers,
                (&capabilities, lifecycle),
            )
            .map_err(Into::into)
        })
        .await
}
