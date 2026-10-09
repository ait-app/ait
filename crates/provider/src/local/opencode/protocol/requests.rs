//! Wire requests and normalized native facts used by the shared turn lifecycle.
use super::{Version, budget, failure, history, http::Api, streaming::Stream};
use crate::local::opencode::{
    OpenCodeExecutionLimits,
    types::{Fault, Invocation, ProtocolError, Record, Snapshot},
};
use reqwest::Method;
use serde_json::{Value, json};
use std::time::Duration;

impl Api {
    /// Generate an input identity accepted by this connection's native protocol.
    pub(in crate::local::opencode) fn new_input_id(&self) -> String {
        self.version.input_id()
    }

    /// Set native agent for `id` when required; v1 selects it on prompt instead.
    /// Propagates a failed native settings request without changing local state.
    pub(in crate::local::opencode) async fn change_agent(
        &self,
        id: &str,
        agent: &str,
    ) -> Result<(), ProtocolError> {
        if self.version == Version::V2 {
            self.json(
                Method::POST,
                &self.path(id, "/agent"),
                Some(&json!({"agent":agent})),
            )
            .await?;
        }
        Ok(())
    }

    /// Submit `request` once to `id`; ambiguous errors never cause a protocol fallback or retry.
    pub(in crate::local::opencode) async fn submit_input(
        &self,
        id: &str,
        request: &Invocation,
    ) -> Result<(), ProtocolError> {
        let body = self.prompt_parameters(request);
        self.json(
            Method::POST,
            &self.path(id, self.prompt_suffix()),
            Some(&body),
        )
        .await?;
        Ok(())
    }

    fn prompt_parameters(&self, request: &Invocation) -> Value {
        match self.version {
            Version::V1 => {
                let model = request
                    .model
                    .split_once('/')
                    .expect("validated provider/model identifier");
                let mut body = json!({"messageID":request.input_id,
                    "parts":[{"type":"text","text":request.prompt}],
                    "agent":request.agent,"model":self.version.model(model, None)});
                if let Some(variant) = &request.reasoning_effort {
                    body["variant"] = json!(variant);
                }
                if let Some(instructions) = &request.instructions {
                    body["system"] = json!(instructions);
                }
                body
            }
            Version::V2 => json!({"text":request.prompt,"files":[],
                "metadata":{"aitInputId":request.input_id}}),
        }
    }

    pub(super) fn prompt_suffix(&self) -> &'static str {
        match self.version {
            Version::V1 => "/prompt_async",
            Version::V2 => "/prompt",
        }
    }

    /// Require the native acknowledgement before reporting an active turn cancelled.
    pub(in crate::local::opencode) async fn interrupt(
        &self,
        id: &str,
    ) -> Result<(), ProtocolError> {
        let suffix = match self.version {
            Version::V1 => "/abort",
            Version::V2 => "/interrupt",
        };
        tokio::time::timeout(
            Duration::from_secs(5),
            self.json(Method::POST, &self.path(id, suffix), Some(&json!({}))),
        )
        .await
        .map_err(|_| failure(Fault::RunRecoveryFailed, "interrupt timed out"))??;
        Ok(())
    }

    /// Validate new native usage against `limits`, excluding `prepared` history.
    /// Returns a limit error without changing or replaying native input.
    pub(in crate::local::opencode) fn validate_budget(
        &self,
        raw: &[Value],
        prepared: &Snapshot,
        limits: OpenCodeExecutionLimits,
    ) -> Result<(), ProtocolError> {
        budget::validate(self.version, raw, prepared, limits)
    }

    /// Decode this protocol's events for `session` and `input` within `limits`.
    pub(in crate::local::opencode) fn stream(
        &self,
        session: &str,
        input: &str,
        limits: OpenCodeExecutionLimits,
    ) -> Stream {
        Stream::for_protocol(self.version, session, input, limits)
    }

    /// Return durable predecessors of canonical text `text` for `input` in `session`.
    /// Returns `None` while unsettled and errors on invalid native history.
    pub(in crate::local::opencode) fn before_text(
        &self,
        session: &str,
        input: &str,
        text: &str,
        messages: &[Value],
    ) -> Result<Option<Vec<Record>>, ProtocolError> {
        history::before_text(self.version, session, input, text, messages)
    }

    /// Borrow permission data from `raw` only when the event belongs to `session`.
    pub(in crate::local::opencode) fn permission_event<'a>(
        raw: &'a Value,
        session: &str,
    ) -> Option<&'a Value> {
        let event = raw.get("payload").unwrap_or(raw);
        let data = event.get("properties").or_else(|| event.get("data"))?;
        (event["type"] == "permission.asked" && data["sessionID"] == session).then_some(data)
    }
}

#[cfg(test)]
mod tests;
