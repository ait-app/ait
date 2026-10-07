//! Native Host model catalog and session-local permission presets.
use super::{
    super::{PROVIDER, config::text},
    http::Api,
};
use crate::{ports::agent_session::AgentSessionError, protocol::provider::Details};
use domain::agent_runtime::{StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};

/// Validate Ait controls before launching a Host; exact choices are checked against native state.
pub(in crate::local::deepseek_harness) fn validate(
    config: &StoredAgentConfig,
) -> Result<(), AgentSessionError> {
    let mut acp = config.clone();
    acp.mode_id = None;
    super::super::config::validate(&acp)?;
    if config
        .mcp_servers
        .as_ref()
        .is_some_and(|servers| !servers.is_empty())
        || config.mode_id.as_ref().is_some_and(|mode| {
            mode.is_empty()
                || mode.len() > 128
                || !mode
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
    {
        return Err(AgentSessionError::Rejected);
    }
    Ok(())
}

/// Resolve current permissions and the native catalog across DSH protocol versions.
/// Older Hosts embed options in the projection; newer Hosts expose a separate catalog.
/// Returns transport/schema errors rather than substituting permissions the Host did not advertise.
pub(super) async fn permission_selection(
    api: &Api,
    projection: &Value,
) -> Result<Value, AgentSessionError> {
    text(projection, "currentValue")?;
    let mut permissions = projection.clone();
    if permissions.get("options").is_none() {
        let catalog = api.call("permissionPresets/catalog", json!({})).await?;
        permissions["options"] = catalog["options"].clone();
    }
    if !permissions["options"].is_array() {
        return Err(AgentSessionError::Failed);
    }
    Ok(permissions)
}

#[derive(Debug)]
pub(super) struct Selection {
    pub(super) catalog: Value,
    pub(super) permissions: Value,
    pub(super) model: Value,
}

impl Selection {
    /// Project a native catalog and the currently composed session presets into Ait's selectors.
    pub(super) fn details(&self) -> Result<Details, AgentSessionError> {
        let mut details = Details::default();
        for group in self.catalog["groups"]
            .as_array()
            .ok_or(AgentSessionError::Failed)?
        {
            let provider = text(group, "id")?;
            for model in group["models"]
                .as_array()
                .ok_or(AgentSessionError::Failed)?
            {
                let id = json!([provider, text(model, "id")?]).to_string();
                let thinking = model["reasoning"]["efforts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|effort| {
                        Ok(json!({"id":text(effort,"id")?,"label":text(effort,"name")?,
                    "isDefault":effort["id"] == model["reasoning"]["defaultEffort"]}))
                    })
                    .collect::<Result<Vec<_>, AgentSessionError>>()?;
                let mut entry = json!({"provider":PROVIDER,"id":id,"label":text(model,"name")?,"isSelectable":true,
                    "isDefault":self.catalog["default"]["provider"] == provider && self.catalog["default"]["model"] == model["id"],
                    "thinkingOptions":thinking});
                if let Some(effort) = model["reasoning"]["defaultEffort"].as_str() {
                    entry["defaultThinkingOptionId"] = json!(effort);
                }
                if let Some(description) = model["description"].as_str() {
                    entry["description"] = json!(description);
                }
                details.models.push(entry);
            }
        }
        for option in self.permissions["options"]
            .as_array()
            .ok_or(AgentSessionError::Failed)?
        {
            let id = text(option, "value")?;
            if id == "custom" {
                continue;
            }
            let mut mode = json!({"id":id,"label":text(option,"name")?});
            if let Some(description) = option["description"].as_str() {
                mode["description"] = json!(description);
            }
            details.modes.push(mode);
        }
        Ok(details)
    }

    /// Apply only explicitly requested, advertised selections; permission scope stays native.
    pub(super) async fn apply(
        &mut self,
        api: &Api,
        session: &str,
        config: &StoredAgentConfig,
    ) -> Result<(), AgentSessionError> {
        validate(config)?;
        let details = self.details()?;
        if config
            .mode_id
            .as_ref()
            .is_some_and(|mode| !details.modes.iter().any(|option| option["id"] == *mode))
        {
            return Err(AgentSessionError::Rejected);
        }
        let id = config
            .model
            .clone()
            .unwrap_or_else(|| json!([self.model["provider"], self.model["model"]]).to_string());
        let selected = details
            .models
            .iter()
            .find(|model| model["id"] == id)
            .ok_or(AgentSessionError::Rejected)?;
        let route: Vec<String> =
            serde_json::from_str(&id).map_err(|_| AgentSessionError::Rejected)?;
        if route.len() != 2 {
            return Err(AgentSessionError::Rejected);
        }
        let mut model = json!({"provider":route[0],"model":route[1]});
        if let Some(effort) = config
            .thinking_option_id
            .as_deref()
            .filter(|effort| !effort.is_empty())
        {
            if !selected["thinkingOptions"]
                .as_array()
                .is_some_and(|options| options.iter().any(|option| option["id"] == effort))
            {
                return Err(AgentSessionError::Rejected);
            }
            model["reasoningEffort"] = json!(effort);
        }
        if config.thinking_option_id.is_none()
            && model["provider"] == self.model["provider"]
            && model["model"] == self.model["model"]
            && let Some(effort) = self.model.get("reasoningEffort")
        {
            model["reasoningEffort"] = effort.clone();
        }
        if model != self.model {
            let mut request = model.clone();
            request["sessionId"] = json!(session);
            let result = api
                .call("session/selectModel", json!({"request":request}))
                .await?;
            let acknowledged = &result["selected"];
            if acknowledged["provider"] != model["provider"]
                || acknowledged["model"] != model["model"]
                || model
                    .get("reasoningEffort")
                    .is_some_and(|effort| acknowledged.get("reasoningEffort") != Some(effort))
                || acknowledged.get("reasoningEffort").is_some_and(|effort| {
                    !selected["thinkingOptions"]
                        .as_array()
                        .is_some_and(|options| options.iter().any(|option| option["id"] == *effort))
                })
            {
                return Err(AgentSessionError::Failed);
            }
            self.model = acknowledged.clone();
        }
        if let Some(mode) = &config.mode_id
            && self.permissions["currentValue"] != *mode
        {
            let result = api.call("commands/execute",json!({"agentId":session,"line":format!("/permission {mode}"),"submittedAttachments":[]})).await?;
            if result["result"]["kind"] != "success" {
                return Err(AgentSessionError::Rejected);
            }
            self.permissions["currentValue"] = json!(mode);
        }
        Ok(())
    }

    /// Return the selections actually acknowledged by the native Host.
    pub(super) fn runtime(&self, session: &str) -> StoredAgentRuntimeInfo {
        StoredAgentRuntimeInfo {
            provider: PROVIDER.into(),
            session_id: Some(session.into()),
            model: Some(json!([self.model["provider"], self.model["model"]]).to_string()),
            thinking_option_id: self.model["reasoningEffort"].as_str().map(str::to_owned),
            mode_id: self.permissions["currentValue"].as_str().map(str::to_owned),
            extra: None,
        }
    }
}
