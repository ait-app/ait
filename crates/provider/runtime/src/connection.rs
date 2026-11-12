//! Connection-owned Timeline observers and plugin display provenance.

use std::collections::{BTreeMap, BTreeSet};

use model::events::Subscription;
use model::outbound::QueueError;
use model::{Context, ErrorCode};
use serde_json::json;
use uuid::Uuid;

use crate::dispatch::State;
use crate::protocol::timeline::SubscriptionRequest;

pub(crate) mod directory;

/// Provider connection state. The diagnostic client label never grants additional authority.
#[derive(Debug, Default)]
pub struct Connection {
    directories: BTreeMap<String, model::polling::Subscription>,
    subscriptions: BTreeMap<String, Subscription>,
    plugin: Option<String>,
}

impl Connection {
    /// Capture plugin item provenance from the trusted-token client's diagnostic label.
    /// This is a namespace convention, not a separately authenticated plugin principal.
    #[must_use]
    pub fn new(client_id: &str) -> Self {
        let plugin = client_id
            .strip_prefix("plugin:")
            .filter(|id| {
                !id.is_empty()
                    && id.len() <= 128
                    && id
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            })
            .map(str::to_owned);
        Self {
            plugin,
            ..Self::default()
        }
    }

    /// Count active observers toward the transport's shared subscription budget.
    #[must_use]
    pub fn len(&self) -> usize {
        self.subscriptions.len() + self.directories.len()
    }

    /// Whether no Provider observers remain.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.subscriptions.is_empty() && self.directories.is_empty()
    }

    /// Stop one observer only if it belongs to this physical connection.
    pub fn release(&mut self, id: &str) {
        self.subscriptions.remove(id);
        self.directories.remove(id);
    }

    pub(crate) fn plugin(&self) -> Option<&str> {
        self.plugin.as_deref()
    }

    pub(crate) async fn create(mut context: Context<'_>, state: &State) -> Result<(), QueueError> {
        if !context.request.params.is_object() {
            return context.respond(Err(ErrorCode::InvalidMessage));
        }
        let observe = context
            .request
            .params
            .get("subscribe")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if observe && context.available_subscriptions == 0 {
            return context.respond(Err(ErrorCode::ResourceExhausted));
        }
        let key = match context.request.params.get("idempotencyKey") {
            Some(serde_json::Value::String(key)) => key.clone(),
            None | Some(serde_json::Value::Null) => Uuid::new_v4().to_string(),
            _ => return context.respond(Err(ErrorCode::InvalidMessage)),
        };
        context.request.params["idempotencyKey"] = json!(key);
        let pending = if observe {
            let Some(execution) = &state.agent_execution else {
                return context.respond(Err(ErrorCode::UnsupportedCapability));
            };
            let creations = execution.creations();
            let outbound = context.outbound.clone();
            // This request is already admitted; a Diff read may temporarily hold the job permit.
            let result = context
                .runtime
                .run_queued(
                    Some(std::sync::Arc::new(std::sync::Mutex::new(creations))),
                    ErrorCode::RegistryIo,
                    move |creations| {
                        creations.observe(domain::creation::protocol::Kind::Agent, &key, outbound)
                    },
                )
                .await;
            match result {
                Ok(subscription) => Some(subscription),
                Err(error) => return context.respond(Err(error)),
            }
        } else {
            None
        };
        if let Some(subscription) = &pending {
            subscription.activate()?;
        }
        let result = super::dispatch::agent_execution::dispatch(
            "agent.create.request",
            std::mem::take(&mut context.request.params),
            state,
        )
        .await;
        match result {
            Ok(value) => {
                context.respond(Ok(value))?;
                Ok(())
            }
            Err(error) => context.respond(Err(error)),
        }
    }

    pub(crate) async fn subscribe(
        &mut self,
        context: Context<'_>,
        state: &State,
    ) -> Result<(), QueueError> {
        let prepared = async {
            let request: SubscriptionRequest =
                serde_json::from_value(context.request.params.clone())
                    .map_err(|_| ErrorCode::InvalidMessage)?;
            if request.agent_ids.len() > 32 {
                return Err(ErrorCode::InvalidMessage);
            }
            if context.available_subscriptions == 0 {
                return Err(ErrorCode::ResourceExhausted);
            }
            let execution = state
                .agent_execution
                .as_ref()
                .ok_or(ErrorCode::UnsupportedCapability)?;
            let resolved = execution
                .execute(
                    "internal.agent.identities.resolve",
                    json!({"agentIds":request.agent_ids}),
                )
                .await?;
            let ids: BTreeSet<String> =
                serde_json::from_value(resolved).map_err(|_| ErrorCode::AgentIo)?;
            let subscription = execution.timeline().events().subscribe(
                Uuid::new_v4().to_string(),
                ids.clone(),
                context.outbound.clone(),
            );
            Ok((ids, subscription))
        }
        .await;
        match prepared {
            Ok((ids, subscription)) => {
                let id = subscription.id().to_owned();
                context.respond(Ok(json!({"subscriptionId":id,"agentIds":ids})))?;
                subscription.activate()?;
                self.subscriptions.insert(id, subscription);
                Ok(())
            }
            Err(error) => context.respond(Err(error)),
        }
    }
}
